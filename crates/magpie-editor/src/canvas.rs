//! Paints the screenshot and its shapes into a GPUI canvas. Geometry comes
//! from `magpie_core::shapes`, the same as the tiny-skia export, so the two
//! agree; only the rasterizer differs.

use std::sync::Arc;

use gpui_kit::{
    App, Bounds, Corners, FontWeight, Hsla, PathBuilder, PathStyle, Pixels, Point as GPoint,
    RenderImage, Rgba, SharedString, StrokeOptions, TextAlign, TextRun, Window, font, point, px,
    size,
};
use lyon::tessellation::{LineCap, LineJoin};
use magpie_core::shapes::{Color, FONT_FAMILY, LINE_HEIGHT, Point, Shape, arrow_geometry};

/// Maps image pixels to window coordinates: the image is fitted into the
/// canvas, centred, and never enlarged past one image pixel per device pixel.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub origin: GPoint<Pixels>,
    pub scale: f32,
}

impl View {
    pub fn fit(canvas: Bounds<Pixels>, image: (u32, u32), device_scale: f32) -> Self {
        let (iw, ih) = (image.0.max(1) as f32, image.1.max(1) as f32);
        let (cw, ch) = (f32::from(canvas.size.width), f32::from(canvas.size.height));
        let scale = (cw / iw).min(ch / ih).min(1.0 / device_scale.max(0.1));
        let origin = point(
            canvas.origin.x + px((cw - iw * scale) / 2.0),
            canvas.origin.y + px((ch - ih * scale) / 2.0),
        );
        View { origin, scale }
    }

    pub fn to_window(self, p: Point) -> GPoint<Pixels> {
        point(
            self.origin.x + px(p.x * self.scale),
            self.origin.y + px(p.y * self.scale),
        )
    }

    pub fn to_image(self, p: GPoint<Pixels>) -> Point {
        Point::new(
            f32::from(p.x - self.origin.x) / self.scale,
            f32::from(p.y - self.origin.y) / self.scale,
        )
    }

    fn len(self, image_px: f32) -> Pixels {
        px(image_px * self.scale)
    }
}

pub fn hsla(c: Color) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
    .into()
}

pub fn paint_image(view: &View, image: &Arc<RenderImage>, dims: (u32, u32), window: &mut Window) {
    let bounds = Bounds {
        origin: view.origin,
        size: size(view.len(dims.0 as f32), view.len(dims.1 as f32)),
    };
    let _ = window.paint_image(bounds, bounds, Corners::default(), image.clone(), 0, false);
}

fn stroke_builder(width: Pixels) -> PathBuilder {
    let options = StrokeOptions::default()
        .with_line_width(f32::from(width))
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    PathBuilder::stroke(width).with_style(PathStyle::Stroke(options))
}

/// Paints one shape. `caret` draws a text cursor after the last character.
pub fn paint_shape(view: &View, shape: &Shape, caret: bool, window: &mut Window, cx: &mut App) {
    match shape {
        Shape::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let ([tip, left, right], shaft_end) = arrow_geometry(*from, *to, *width);
            let mut shaft = stroke_builder(view.len(*width));
            shaft.move_to(view.to_window(*from));
            shaft.line_to(view.to_window(shaft_end));
            if let Ok(path) = shaft.build() {
                window.paint_path(path, hsla(*color));
            }
            let mut head = PathBuilder::fill();
            head.move_to(view.to_window(tip));
            head.line_to(view.to_window(left));
            head.line_to(view.to_window(right));
            head.close();
            if let Ok(path) = head.build() {
                window.paint_path(path, hsla(*color));
            }
        }
        Shape::Rect {
            from,
            to,
            color,
            width,
        } => {
            let (a, b) = (view.to_window(*from), view.to_window(*to));
            let mut path = stroke_builder(view.len(*width));
            path.move_to(a);
            path.line_to(point(b.x, a.y));
            path.line_to(b);
            path.line_to(point(a.x, b.y));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, hsla(*color));
            }
        }
        Shape::Ellipse {
            from,
            to,
            color,
            width,
        } => {
            let (a, b) = (view.to_window(*from), view.to_window(*to));
            let (rx, ry) = ((b.x - a.x).abs() / 2.0, (b.y - a.y).abs() / 2.0);
            if rx <= px(0.5) || ry <= px(0.5) {
                return;
            }
            let center = point((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let mut path = stroke_builder(view.len(*width));
            path.move_to(point(center.x + rx, center.y));
            path.arc_to(
                point(rx, ry),
                px(0.),
                false,
                false,
                point(center.x - rx, center.y),
            );
            path.arc_to(
                point(rx, ry),
                px(0.),
                false,
                false,
                point(center.x + rx, center.y),
            );
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, hsla(*color));
            }
        }
        Shape::Text {
            at,
            text,
            color,
            size,
        } => paint_text(view, *at, text, *color, *size, caret, window, cx),
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_text(
    view: &View,
    at: Point,
    text: &str,
    color: Color,
    size: f32,
    caret: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let font_size = view.len(size);
    let line_height = view.len(size * LINE_HEIGHT);
    let mut font = font(FONT_FAMILY);
    font.weight = FontWeight::BOLD;
    let mut end = view.to_window(at);
    for (row, line) in text.split('\n').enumerate() {
        let origin = view.to_window(Point::new(at.x, at.y + row as f32 * size * LINE_HEIGHT));
        let run = TextRun {
            len: line.len(),
            font: font.clone(),
            color: hsla(color),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            SharedString::from(line.to_string()),
            font_size,
            &[run],
            None,
        );
        let _ = shaped.paint(origin, line_height, TextAlign::Left, None, window, cx);
        end = point(origin.x + shaped.width, origin.y);
    }
    if caret {
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(point(end.x + px(2.), end.y + line_height * 0.15));
        path.line_to(point(end.x + px(2.), end.y + line_height * 0.85));
        if let Ok(path) = path.build() {
            window.paint_path(path, hsla(color));
        }
    }
}

/// Dashed outline around a selected shape.
pub fn paint_selection(
    view: &View,
    bounds: magpie_core::shapes::Bounds,
    accent: Hsla,
    window: &mut Window,
) {
    let pad = 6.0 / view.scale;
    let a = view.to_window(Point::new(bounds.left - pad, bounds.top - pad));
    let b = view.to_window(Point::new(bounds.right + pad, bounds.bottom + pad));
    let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(5.), px(4.)]);
    path.move_to(a);
    path.line_to(point(b.x, a.y));
    path.line_to(b);
    path.line_to(point(a.x, b.y));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, accent);
    }
}
