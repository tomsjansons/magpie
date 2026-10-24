//! The editor window: a toolbar over a canvas showing the screenshot with its
//! shapes and frame. Every edit replaces the annotation wholesale and pushes
//! the previous one on the undo stack, which keeps undo trivially correct.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::{
    App, Bounds, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Render, RenderImage, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, canvas, div, linear_color_stop, linear_gradient, px,
};
use magpie_core::frame::{Frame, GRADIENTS, PADDINGS};
use magpie_core::shapes::{Annotation, Color, Point, Shape};
use magpie_core::{Library, ORIGINAL, clipboard, render};

use crate::canvas::{self, View};
use crate::icons::Icon;

/// Palette offered in the toolbar; the first is the default.
const PALETTE: [(&str, Color); 7] = [
    ("Red", Color::rgb(0xef, 0x44, 0x44)),
    ("Orange", Color::rgb(0xf9, 0x73, 0x16)),
    ("Yellow", Color::rgb(0xfa, 0xcc, 0x15)),
    ("Green", Color::rgb(0x22, 0xc5, 0x5e)),
    ("Blue", Color::rgb(0x3b, 0x82, 0xf6)),
    ("Black", Color::rgb(0x11, 0x11, 0x11)),
    ("White", Color::rgb(0xff, 0xff, 0xff)),
];

/// Stroke width multipliers for the S / M / L buttons.
const WIDTHS: [(&str, f32); 3] = [("S", 0.5), ("M", 1.0), ("L", 2.0)];

/// Text size as a multiple of the stroke width.
const TEXT_PER_WIDTH: f32 = 6.0;

/// Pointer slack for picking shapes, in window pixels.
const HIT_SLOP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Arrow,
    Rect,
    Ellipse,
    Text,
}

impl Tool {
    const ALL: [(Tool, Icon, &'static str); 5] = [
        (Tool::Select, Icon::MousePointer, "Select and move (V)"),
        (Tool::Arrow, Icon::Arrow, "Arrow (A)"),
        (Tool::Rect, Icon::Rect, "Box (R)"),
        (Tool::Ellipse, Icon::Ellipse, "Circle (E)"),
        (Tool::Text, Icon::Text, "Text (T)"),
    ];
}

enum Drag {
    /// Drawing a new arrow, box or circle.
    Draw { start: Point, current: Point },
    /// Moving the selected shape; `before` is what to undo to.
    Move {
        last: Point,
        before: Annotation,
        moved: bool,
    },
}

/// Text being typed. `index` is the shape being edited, or `None` for a new
/// one that only lands in `shapes` when it is committed.
struct TextEdit {
    index: Option<usize>,
    at: Point,
    text: String,
    color: Color,
    size: f32,
}

pub struct Editor {
    lib: Arc<Library>,
    shot_id: String,
    /// The version new saves are based on. Starts as the version the editor
    /// was opened on and follows each save.
    parent: String,
    image: Arc<RenderImage>,
    dims: (u32, u32),
    base_width: f32,

    shapes: Vec<Shape>,
    frame: Option<Frame>,
    /// The frame switching the frame back on restores.
    last_frame: Frame,
    undo: Vec<Annotation>,
    redo: Vec<Annotation>,
    dirty: bool,

    tool: Tool,
    color: Color,
    width_factor: f32,
    selected: Option<usize>,
    drag: Option<Drag>,
    editing: Option<TextEdit>,

    status: Option<SharedString>,
    busy: bool,
    view: Rc<Cell<Option<View>>>,
    focus: FocusHandle,
}

impl Editor {
    pub fn new(
        lib: Arc<Library>,
        shot_id: String,
        parent: String,
        annotation: Annotation,
        image: Arc<RenderImage>,
        dims: (u32, u32),
        cx: &mut Context<Self>,
    ) -> Self {
        // Scale defaults to the image: ~10 px strokes on a 4K shot, 3 px minimum.
        let base_width = (dims.0.max(dims.1) as f32 / 400.0).round().clamp(3.0, 16.0);
        Editor {
            lib,
            shot_id,
            parent,
            image,
            dims,
            base_width,
            shapes: annotation.shapes,
            frame: annotation.frame,
            last_frame: annotation
                .frame
                .unwrap_or_else(|| Frame::new(dims, 0, PADDINGS[1].1)),
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            tool: Tool::Arrow,
            color: PALETTE[0].1,
            width_factor: 1.0,
            selected: None,
            drag: None,
            editing: None,
            status: None,
            busy: false,
            view: Rc::new(Cell::new(None)),
            focus: cx.focus_handle(),
        }
    }

    pub fn title(&self) -> String {
        if self.parent == ORIGINAL {
            format!("magpie · {}", self.shot_id)
        } else {
            format!("magpie · {} from {}", self.shot_id, self.parent)
        }
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.dirty && (!self.shapes.is_empty() || self.frame.is_some())
    }

    fn stroke_width(&self) -> f32 {
        (self.base_width * self.width_factor).max(1.0)
    }

    fn text_size(&self) -> f32 {
        self.stroke_width() * TEXT_PER_WIDTH
    }

    // ── Edits ────────────────────────────────────────────────────────────────

    fn checkpoint(&mut self, before: Annotation) {
        self.undo.push(before);
        self.redo.clear();
        self.dirty = true;
        self.status = None;
    }

    fn set_shapes(&mut self, shapes: Vec<Shape>) {
        let before = self.annotation();
        self.shapes = shapes;
        self.checkpoint(before);
    }

    fn set_frame(&mut self, frame: Option<Frame>) {
        if frame == self.frame {
            return;
        }
        let before = self.annotation();
        if let Some(frame) = frame {
            self.last_frame = frame;
        }
        self.frame = frame;
        self.checkpoint(before);
    }

    /// Replaces the current state with `annotation`, returning the old one.
    fn restore(&mut self, annotation: Annotation) -> Annotation {
        let current = self.annotation();
        self.shapes = annotation.shapes;
        self.frame = annotation.frame;
        if let Some(frame) = self.frame {
            self.last_frame = frame;
        }
        self.selected = None;
        self.dirty = true;
        current
    }

    fn undo(&mut self) {
        self.commit_text();
        if let Some(previous) = self.undo.pop() {
            let current = self.restore(previous);
            self.redo.push(current);
        }
    }

    fn redo(&mut self) {
        self.commit_text();
        if let Some(next) = self.redo.pop() {
            let current = self.restore(next);
            self.undo.push(current);
        }
    }

    fn delete_selected(&mut self) {
        if let Some(index) = self.selected.take() {
            let mut shapes = self.shapes.clone();
            shapes.remove(index);
            self.set_shapes(shapes);
        }
    }

    /// Apply the selected color or width to the selected shape too.
    fn restyle_selected(&mut self) {
        let Some(index) = self.selected else { return };
        let (color, width, size) = (self.color, self.stroke_width(), self.text_size());
        let mut shapes = self.shapes.clone();
        match &mut shapes[index] {
            Shape::Arrow {
                color: c, width: w, ..
            }
            | Shape::Rect {
                color: c, width: w, ..
            }
            | Shape::Ellipse {
                color: c, width: w, ..
            } => {
                *c = color;
                *w = width;
            }
            Shape::Text {
                color: c, size: s, ..
            } => {
                *c = color;
                *s = size;
            }
        }
        if shapes != self.shapes {
            self.set_shapes(shapes);
        }
    }

    fn start_text(&mut self, at: Point, existing: Option<usize>) {
        self.commit_text();
        let edit = match existing.map(|i| (i, &self.shapes[i])) {
            Some((
                index,
                Shape::Text {
                    at,
                    text,
                    color,
                    size,
                },
            )) => TextEdit {
                index: Some(index),
                at: *at,
                text: text.clone(),
                color: *color,
                size: *size,
            },
            _ => TextEdit {
                index: None,
                at,
                text: String::new(),
                color: self.color,
                size: self.text_size(),
            },
        };
        self.selected = None;
        self.editing = Some(edit);
    }

    fn commit_text(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        let text = edit.text.trim_end().to_string();
        let mut shapes = self.shapes.clone();
        let shape = Shape::Text {
            at: edit.at,
            text: text.clone(),
            color: edit.color,
            size: edit.size,
        };
        match (edit.index, text.is_empty()) {
            (Some(index), true) => {
                shapes.remove(index);
            }
            (Some(index), false) => shapes[index] = shape,
            (None, true) => return,
            (None, false) => shapes.push(shape),
        }
        if shapes != self.shapes {
            self.set_shapes(shapes);
        }
    }

    fn annotation(&self) -> Annotation {
        Annotation {
            shapes: self.shapes.clone(),
            frame: self.frame,
        }
    }

    // ── Save and copy ────────────────────────────────────────────────────────

    /// Saves a new version, then optionally copies it. Rendering a 4K shot
    /// takes a moment, so it runs off the UI thread.
    fn save(&mut self, copy: bool, cx: &mut Context<Self>) {
        self.commit_text();
        if self.busy {
            return;
        }
        if !self.dirty && copy && self.parent != ORIGINAL {
            // Nothing new to save: copy the version we are on.
            return self.copy_saved(cx);
        }
        self.busy = true;
        self.status = Some("Saving…".into());
        let (lib, shot, parent, annotation) = (
            self.lib.clone(),
            self.shot_id.clone(),
            self.parent.clone(),
            self.annotation(),
        );
        let task = cx.background_executor().spawn(async move {
            let variant = lib.save_variant(&shot, &parent, &annotation)?;
            if copy {
                let target = format!("{shot}@{}", variant.id)
                    .parse()
                    .map_err(anyhow::Error::msg)?;
                clipboard::copy_png(&lib.resolve(&target)?)?;
            }
            anyhow::Ok(variant.id)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(id) => {
                        this.status = Some(
                            if copy {
                                format!("Saved {id} and copied")
                            } else {
                                format!("Saved {id}")
                            }
                            .into(),
                        );
                        this.parent = id;
                        this.dirty = false;
                    }
                    Err(err) => this.status = Some(format!("Save failed: {err:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_saved(&mut self, cx: &mut Context<Self>) {
        let target = format!("{}@{}", self.shot_id, self.parent);
        let result = target
            .parse()
            .map_err(anyhow::Error::msg)
            .and_then(|t| self.lib.resolve(&t))
            .and_then(|path| clipboard::copy_png(&path));
        self.status = Some(match result {
            Ok(()) => format!("Copied {}", self.parent).into(),
            Err(err) => format!("Copy failed: {err:#}").into(),
        });
        cx.notify();
    }

    /// Copies what is on screen without saving a version.
    fn copy(&mut self, cx: &mut Context<Self>) {
        self.commit_text();
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = Some("Copying…".into());
        let (lib, shot, annotation) = (self.lib.clone(), self.shot_id.clone(), self.annotation());
        let task = cx.background_executor().spawn(async move {
            let shot = lib.shot(&shot)?;
            let base = image::open(lib.original_path(&shot))?.to_rgba8();
            let png = render::encode_png(&render::render(&base, &annotation)?)?;
            clipboard::copy_png_bytes(&png)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.status = Some(match result {
                    Ok(()) => "Copied".into(),
                    Err(err) => format!("Copy failed: {err:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Synchronous save for window close, so unsaved work is never dropped.
    pub fn save_blocking(&mut self) {
        self.commit_text();
        if self.has_unsaved_changes() {
            if let Err(err) = self
                .lib
                .save_variant(&self.shot_id, &self.parent, &self.annotation())
            {
                eprintln!("magpie: saving on close failed: {err:#}");
            }
            self.dirty = false;
        }
    }

    // ── Pointer ──────────────────────────────────────────────────────────────

    fn hit(&self, p: Point) -> Option<usize> {
        let view = self.view.get()?;
        let slop = HIT_SLOP / view.scale;
        self.shapes.iter().rposition(|s| s.hit(p, slop))
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(view) = self.view.get() else { return };
        let p = view.to_image(ev.position);
        match self.tool {
            Tool::Select => {
                self.commit_text();
                self.selected = self.hit(p);
                if let Some(index) = self.selected {
                    if ev.click_count >= 2 && matches!(self.shapes[index], Shape::Text { .. }) {
                        self.start_text(p, Some(index));
                    } else {
                        self.drag = Some(Drag::Move {
                            last: p,
                            before: self.annotation(),
                            moved: false,
                        });
                    }
                }
            }
            Tool::Text => {
                let existing = self
                    .hit(p)
                    .filter(|&i| matches!(self.shapes[i], Shape::Text { .. }));
                self.start_text(p, existing);
            }
            Tool::Arrow | Tool::Rect | Tool::Ellipse => {
                self.commit_text();
                self.selected = None;
                self.drag = Some(Drag::Draw {
                    start: p,
                    current: p,
                });
            }
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.view.get() else { return };
        let p = view.to_image(ev.position);
        match &mut self.drag {
            Some(Drag::Draw { current, .. }) => *current = p,
            Some(Drag::Move { last, moved, .. }) => {
                if let Some(index) = self.selected {
                    self.shapes[index].translate(p.x - last.x, p.y - last.y);
                    *last = p;
                    *moved = true;
                }
            }
            None => return,
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _ev: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Draw { start, current }) => {
                if let Some(shape) = self.draft(start, current) {
                    let mut shapes = self.shapes.clone();
                    shapes.push(shape);
                    self.set_shapes(shapes);
                }
            }
            Some(Drag::Move {
                before,
                moved: true,
                ..
            }) => self.checkpoint(before),
            _ => {}
        }
        cx.notify();
    }

    /// The shape a drag from `start` to `end` makes with the current tool, or
    /// `None` for a drag too short to mean anything.
    fn draft(&self, start: Point, end: Point) -> Option<Shape> {
        let min = self.view.get().map_or(4.0, |v| 4.0 / v.scale);
        if (end.x - start.x).abs() < min && (end.y - start.y).abs() < min {
            return None;
        }
        let (color, width) = (self.color, self.stroke_width());
        Some(match self.tool {
            Tool::Arrow => Shape::Arrow {
                from: start,
                to: end,
                color,
                width,
            },
            Tool::Rect => Shape::Rect {
                from: start,
                to: end,
                color,
                width,
            },
            Tool::Ellipse => Shape::Ellipse {
                from: start,
                to: end,
                color,
                width,
            },
            Tool::Select | Tool::Text => return None,
        })
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let key = ks.key.as_str();

        if m.control {
            match (key, m.shift) {
                ("z", false) => self.undo(),
                ("z", true) | ("y", _) => self.redo(),
                ("c", false) => self.copy(cx),
                ("c", true) => self.save(true, cx),
                ("s", _) => self.save(false, cx),
                ("w" | "q", _) => window.remove_window(),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        if let Some(edit) = &mut self.editing {
            match key {
                "escape" => self.commit_text(),
                "enter" if m.shift => edit.text.push('\n'),
                "enter" => self.commit_text(),
                "backspace" => {
                    edit.text.pop();
                }
                _ => match &ks.key_char {
                    Some(ch) if !m.alt && !m.platform => edit.text.push_str(ch),
                    _ => return,
                },
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        match key {
            "v" => self.tool = Tool::Select,
            "a" => self.tool = Tool::Arrow,
            "r" => self.tool = Tool::Rect,
            "e" | "c" => self.tool = Tool::Ellipse,
            "t" => self.tool = Tool::Text,
            "delete" | "backspace" => self.delete_selected(),
            "escape" => {
                self.selected = None;
                self.drag = None;
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let mut tools = div().flex().gap_1();
        for (tool, icon, tip) in Tool::ALL {
            tools = tools.child(
                Button::new(SharedString::from(format!("tool-{tip}")))
                    .icon(icon)
                    .ghost()
                    .small()
                    .selected(self.tool == tool)
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.commit_text();
                        this.tool = tool;
                        if tool != Tool::Select {
                            this.selected = None;
                        }
                        cx.notify();
                    })),
            );
        }

        let mut colors = div().flex().gap_1().items_center();
        for (name, color) in PALETTE {
            let active = self.color == color;
            colors = colors.child(
                div()
                    .id(SharedString::from(format!("color-{name}")))
                    .size(px(20.))
                    .rounded_full()
                    .bg(canvas::hsla(color))
                    .border_2()
                    .border_color(if active { theme.foreground } else { border })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.color = color;
                        if let Some(edit) = &mut this.editing {
                            edit.color = color;
                        }
                        this.restyle_selected();
                        cx.notify();
                    })),
            );
        }

        let mut widths = div().flex().gap_1();
        for (label, factor) in WIDTHS {
            widths = widths.child(
                Button::new(SharedString::from(format!("width-{label}")))
                    .label(label)
                    .ghost()
                    .small()
                    .selected(self.width_factor == factor)
                    .tooltip("Line width and text size")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.width_factor = factor;
                        let size = this.text_size();
                        if let Some(edit) = &mut this.editing {
                            edit.size = size;
                        }
                        this.restyle_selected();
                        cx.notify();
                    })),
            );
        }

        let frame_toggle = Button::new("frame")
            .icon(Icon::Frame)
            .label("Background")
            .ghost()
            .small()
            .selected(self.frame.is_some())
            .tooltip("Gradient background with rounded corners")
            .on_click(cx.listener(|this, _, _, cx| {
                this.commit_text();
                let frame = match this.frame {
                    Some(_) => None,
                    None => Some(this.last_frame),
                };
                this.set_frame(frame);
                cx.notify();
            }));

        let actions = div()
            .flex()
            .gap_1()
            .items_center()
            .child(
                Button::new("undo")
                    .icon(Icon::Undo)
                    .ghost()
                    .small()
                    .disabled(self.undo.is_empty())
                    .tooltip("Undo (Ctrl+Z)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.undo();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("redo")
                    .icon(Icon::Redo)
                    .ghost()
                    .small()
                    .disabled(self.redo.is_empty())
                    .tooltip("Redo (Ctrl+Shift+Z)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.redo();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("delete")
                    .icon(Icon::Trash)
                    .ghost()
                    .small()
                    .disabled(self.selected.is_none())
                    .tooltip("Delete selected (Del)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.delete_selected();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("copy")
                    .icon(Icon::Copy)
                    .label("Copy")
                    .small()
                    .disabled(self.busy)
                    .tooltip("Copy to clipboard without saving (Ctrl+C)")
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
            )
            .child(
                Button::new("save")
                    .icon(Icon::Save)
                    .label("Save version")
                    .primary()
                    .small()
                    .disabled(self.busy || !self.dirty)
                    .tooltip("Save as a new version (Ctrl+S); Ctrl+Shift+C saves and copies")
                    .on_click(cx.listener(|this, _, _, cx| this.save(false, cx))),
            );

        let status = self.status.clone().unwrap_or_else(|| {
            if self.dirty {
                "Unsaved changes".into()
            } else if self.parent == ORIGINAL {
                "Original".into()
            } else {
                format!("On {}", self.parent).into()
            }
        });

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .bg(theme.background)
            .child(tools)
            .child(div().w(px(1.)).h(px(20.)).bg(border))
            .child(colors)
            .child(div().w(px(1.)).h(px(20.)).bg(border))
            .child(widths)
            .child(div().w(px(1.)).h(px(20.)).bg(border))
            .child(frame_toggle)
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .text_right()
                    .child(status),
            )
            .child(actions)
    }
}

impl Editor {
    /// Background presets and padding, shown while the frame is on.
    fn frame_bar(&self, frame: Frame, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let mut gradients = div().flex().gap_1().items_center();
        for (index, (name, from, to)) in GRADIENTS.iter().copied().enumerate() {
            let active = frame.uses_gradient(index);
            gradients = gradients.child(
                div()
                    .id(SharedString::from(format!("gradient-{name}")))
                    .size(px(20.))
                    .rounded_full()
                    .bg(linear_gradient(
                        frame.angle,
                        linear_color_stop(canvas::hsla(from), 0.),
                        linear_color_stop(canvas::hsla(to), 1.),
                    ))
                    .border_2()
                    .border_color(if active { theme.foreground } else { border })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(frame) = this.frame {
                            this.set_frame(Some(Frame { from, to, ..frame }));
                        }
                        cx.notify();
                    })),
            );
        }

        let long = self.dims.0.max(self.dims.1) as f32;
        let mut paddings = div().flex().gap_1();
        for (label, fraction) in PADDINGS {
            let padding = (long * fraction).round();
            paddings = paddings.child(
                Button::new(SharedString::from(format!("padding-{label}")))
                    .label(label)
                    .ghost()
                    .small()
                    .selected(frame.padding == padding)
                    .tooltip("Padding around the screenshot")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(frame) = this.frame {
                            this.set_frame(Some(Frame { padding, ..frame }));
                        }
                        cx.notify();
                    })),
            );
        }

        let label = |text: &'static str| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(border)
            .bg(theme.background)
            .child(label("Background"))
            .child(gradients)
            .child(div().w(px(1.)).h(px(20.)).bg(border))
            .child(label("Padding"))
            .child(paddings)
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Everything the canvas paints, in paint order.
        let mut shapes: Vec<(Shape, bool)> = Vec::with_capacity(self.shapes.len() + 1);
        for (index, shape) in self.shapes.iter().enumerate() {
            if self
                .editing
                .as_ref()
                .is_some_and(|e| e.index == Some(index))
            {
                continue;
            }
            shapes.push((shape.clone(), false));
        }
        if let Some(Drag::Draw { start, current }) = &self.drag
            && let Some(draft) = self.draft(*start, *current)
        {
            shapes.push((draft, false));
        }
        if let Some(edit) = &self.editing {
            shapes.push((
                Shape::Text {
                    at: edit.at,
                    text: edit.text.clone(),
                    color: edit.color,
                    size: edit.size,
                },
                true,
            ));
        }
        let selection = self
            .selected
            .and_then(|i| self.shapes.get(i))
            .map(Shape::bounds);
        let accent = cx.theme().primary;
        let (image, dims, frame) = (self.image.clone(), self.dims, self.frame);
        let view_cell = self.view.clone();
        let device_scale = window.scale_factor();
        let cursor = match self.tool {
            Tool::Select => gpui_kit::CursorStyle::Arrow,
            Tool::Text => gpui_kit::CursorStyle::IBeam,
            _ => gpui_kit::CursorStyle::Crosshair,
        };

        div()
            .id("editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.toolbar(cx))
            .children(frame.map(|frame| self.frame_bar(frame, cx)))
            .child(
                div()
                    .id("canvas")
                    .flex_1()
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .cursor(cursor)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .child(
                        canvas(
                            move |bounds: Bounds<Pixels>, _, _| {
                                let pad = frame.map_or(0, |f| f.pad());
                                let view = View::fit(bounds, dims, pad, device_scale);
                                view_cell.set(Some(view));
                                view
                            },
                            move |_, view, window, cx| {
                                if let Some(frame) = &frame {
                                    canvas::paint_frame(&view, frame, dims, window);
                                }
                                let radius = frame.map_or(0., |f| f.radius);
                                canvas::paint_image(&view, &image, dims, radius, window);
                                for (shape, caret) in &shapes {
                                    canvas::paint_shape(&view, shape, *caret, window, cx);
                                }
                                if let Some(bounds) = selection {
                                    canvas::paint_selection(&view, bounds, accent, window);
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}
