//! The GPUI annotation editor.

mod canvas;
mod editor;
mod icons;

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui_kit::component::Theme;
use gpui_kit::{
    App, AppContext as _, Bounds, Focusable as _, RenderImage, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};
use magpie_core::shapes::FONT_BYTES;
use magpie_core::{Library, Target};
use smallvec::SmallVec;

use crate::editor::Editor;

/// Opens the editor on `target` and blocks until its window closes. Edits
/// start from that version's shapes; saving adds a version whose parent is
/// `target`'s version.
pub fn run(lib: Library, target: Target) -> Result<()> {
    let shot = lib.shot(&target.shot)?;
    let annotation = lib.annotation(&target)?;
    let original = lib.original_path(&shot);
    let base = image::open(&original)
        .with_context(|| format!("opening {}", original.display()))?
        .to_rgba8();
    let dims = base.dimensions();
    let image = Arc::new(render_image(base));
    let lib = Arc::new(lib);

    gpui_kit::application()
        .with_assets(icons::AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            if let Err(err) = cx.text_system().add_fonts(vec![Cow::Borrowed(FONT_BYTES)]) {
                eprintln!("magpie: loading the annotation font failed: {err:#}");
            }

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1400.), px(900.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("magpie · {}", target.shot).into()),
                    ..Default::default()
                }),
                app_id: Some("magpie".into()),
                focus: true,
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                // On Linux the portal's colour scheme arrives after start-up,
                // so follow the window's appearance rather than the app's.
                Theme::sync_system_appearance(Some(window), cx);
                window
                    .observe_window_appearance(|window, cx| {
                        Theme::sync_system_appearance(Some(window), cx)
                    })
                    .detach();
                let editor = cx.new(|cx| {
                    Editor::new(
                        lib.clone(),
                        shot.id.clone(),
                        target.version.clone(),
                        annotation.clone(),
                        image.clone(),
                        dims,
                        cx,
                    )
                });
                let title = editor.read(cx).title();
                window.set_window_title(&title);
                window.focus(&editor.read(cx).focus_handle(cx), cx);

                let on_close = editor.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    on_close.update(cx, |editor, _| editor.save_blocking());
                    true
                });
                editor
            });
            if let Err(err) = opened {
                eprintln!("magpie: opening the editor window failed: {err:#}");
                cx.quit();
                return;
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
    Ok(())
}

/// GPUI wants BGRA frames.
fn render_image(mut rgba: image::RgbaImage) -> RenderImage {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let frame = image::Frame::new(rgba);
    RenderImage::new(SmallVec::from_elem(frame, 1))
}
