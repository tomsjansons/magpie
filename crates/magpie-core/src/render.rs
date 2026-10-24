//! Draws annotations onto a screenshot with tiny-skia. This is the export
//! path: what gets saved and copied never depends on the editor's renderer.

use std::io::Cursor;

use ab_glyph::{Font, OutlineCurve};
use anyhow::{Context, Result};
use image::{ImageFormat, RgbaImage};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint,
    Rect, SpreadMode, Stroke, Transform,
};

use crate::frame::{Frame, SHADOW_OPACITY};
use crate::shapes::{Annotation, Color, Point, Shape, arrow_geometry, font, layout_text};

/// `base` with the annotation's shapes painted on top, in order, inside its
/// frame when it has one.
pub fn render(base: &RgbaImage, annotation: &Annotation) -> Result<RgbaImage> {
    let image = to_pixmap(base)?;
    let mut pixmap = match &annotation.frame {
        Some(frame) => framed(&image, frame)?,
        None => image,
    };

    let pad = annotation.frame.map_or(0.0, |f| f.pad() as f32);
    let transform = Transform::from_translate(pad, pad);
    for shape in &annotation.shapes {
        draw(&mut pixmap, shape, transform);
    }

    let mut out = RgbaImage::new(pixmap.width(), pixmap.height());
    for (dst, src) in out.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    Ok(out)
}

fn to_pixmap(image: &RgbaImage) -> Result<Pixmap> {
    let (w, h) = image.dimensions();
    let mut pixmap = Pixmap::new(w, h).context("image has no pixels")?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Ok(pixmap)
}

/// The gradient background with the shadow and the rounded screenshot on it.
fn framed(image: &Pixmap, frame: &Frame) -> Result<Pixmap> {
    let (w, h) = frame.outer((image.width(), image.height()));
    let mut pixmap = Pixmap::new(w, h).context("frame has no pixels")?;
    let (fw, fh) = (w as f32, h as f32);
    for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
        let c = frame.color_at(x, y, fw, fh);
        *px = tiny_skia::ColorU8::from_rgba(c.r, c.g, c.b, c.a).premultiply();
    }

    let pad = frame.pad() as f32;
    let (iw, ih) = (image.width() as f32, image.height() as f32);
    let radius = frame.radius.min(iw / 2.0).min(ih / 2.0).max(0.0);

    if frame.shadow > 0.0 {
        let offset = frame.shadow_offset();
        let mut shadow = Pixmap::new(w, h).context("frame has no pixels")?;
        if let Some(path) = rounded_rect(pad + offset.x, pad + offset.y, iw, ih, radius) {
            shadow.fill_path(
                &path,
                &paint(Color::rgb(0, 0, 0)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        blur_alpha(&mut shadow, frame.shadow);
        pixmap.draw_pixmap(
            0,
            0,
            shadow.as_ref(),
            &PixmapPaint {
                opacity: SHADOW_OPACITY,
                ..PixmapPaint::default()
            },
            Transform::identity(),
            None,
        );
    }

    if let Some(path) = rounded_rect(pad, pad, iw, ih, radius) {
        let paint = Paint {
            shader: Pattern::new(
                image.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Nearest,
                1.0,
                Transform::from_translate(pad, pad),
            ),
            anti_alias: true,
            ..Paint::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    Ok(pixmap)
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if r <= 0.0 {
        return Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect);
    }
    // Control point distance for a quarter circle drawn as a cubic.
    let k = r * 0.552_284_8;
    let (right, bottom) = (x + w, y + h);
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(right - r, y);
    p.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    p.line_to(right, bottom - r);
    p.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    p.line_to(x + r, bottom);
    p.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish()
}

/// Gaussian blur of a black mask, approximated by three box blurs. Only the
/// alpha channel carries anything; colour stays black.
fn blur_alpha(pixmap: &mut Pixmap, sigma: f32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // Three passes of a box of width b have variance 3 * (b^2 - 1) / 12.
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round() as usize;
    if radius == 0 {
        return;
    }
    let mut alpha: Vec<u16> = pixmap.pixels().iter().map(|p| p.alpha() as u16).collect();
    let mut scratch = vec![0u16; alpha.len()];
    for _ in 0..3 {
        box_pass(&alpha, &mut scratch, w, h, radius, 1, w);
        box_pass(&scratch, &mut alpha, h, w, radius, w, 1);
    }
    for (px, a) in pixmap.pixels_mut().iter_mut().zip(alpha) {
        *px = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, a.min(255) as u8)
            .expect("black is valid at any alpha");
    }
}

/// One box blur along lines of `len` samples `step` apart, `lines` lines
/// `stride` apart. Samples past the edges count as transparent.
fn box_pass(
    src: &[u16],
    dst: &mut [u16],
    len: usize,
    lines: usize,
    radius: usize,
    step: usize,
    stride: usize,
) {
    let width = (2 * radius + 1) as u32;
    for line in 0..lines {
        let at = |i: usize| line * stride + i * step;
        let mut sum: u32 = (0..radius.min(len)).map(|i| src[at(i)] as u32).sum();
        for i in 0..len {
            if i + radius < len {
                sum += src[at(i + radius)] as u32;
            }
            dst[at(i)] = ((sum + width / 2) / width) as u16;
            if i >= radius {
                sum -= src[at(i - radius)] as u32;
            }
        }
    }
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn rect(from: Point, to: Point) -> Option<Rect> {
    Rect::from_ltrb(
        from.x.min(to.x),
        from.y.min(to.y),
        from.x.max(to.x),
        from.y.max(to.y),
    )
}

fn draw(pixmap: &mut Pixmap, shape: &Shape, id: Transform) {
    match shape {
        Shape::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let ([tip, left, right], shaft_end) = arrow_geometry(*from, *to, *width);
            let mut shaft = PathBuilder::new();
            shaft.move_to(from.x, from.y);
            shaft.line_to(shaft_end.x, shaft_end.y);
            if let Some(path) = shaft.finish() {
                pixmap.stroke_path(&path, &paint(*color), &stroke(*width), id, None);
            }
            let mut head = PathBuilder::new();
            head.move_to(tip.x, tip.y);
            head.line_to(left.x, left.y);
            head.line_to(right.x, right.y);
            head.close();
            if let Some(path) = head.finish() {
                pixmap.fill_path(&path, &paint(*color), FillRule::Winding, id, None);
            }
        }
        Shape::Rect {
            from,
            to,
            color,
            width,
        } => {
            if let Some(path) = rect(*from, *to).map(PathBuilder::from_rect) {
                pixmap.stroke_path(&path, &paint(*color), &stroke(*width), id, None);
            }
        }
        Shape::Ellipse {
            from,
            to,
            color,
            width,
        } => {
            if let Some(path) = rect(*from, *to).and_then(PathBuilder::from_oval) {
                pixmap.stroke_path(&path, &paint(*color), &stroke(*width), id, None);
            }
        }
        Shape::Text {
            at,
            text,
            color,
            size,
        } => draw_text(pixmap, *at, text, *color, *size, id),
    }
}

/// Fills each glyph outline as a path, so text antialiases like the shapes.
fn draw_text(
    pixmap: &mut Pixmap,
    at: Point,
    text: &str,
    color: Color,
    size: f32,
    transform: Transform,
) {
    let font = font();
    let layout = layout_text(text, size);
    let s = layout.scale;
    let mut path = PathBuilder::new();
    for glyph in &layout.glyphs {
        let Some(outline) = font.outline(glyph.id) else {
            continue;
        };
        let (ox, oy) = (at.x + glyph.x, at.y + glyph.baseline);
        // Font units have y pointing up.
        let p = |pt: ab_glyph::Point| (ox + pt.x * s, oy - pt.y * s);
        let mut last: Option<ab_glyph::Point> = None;
        for curve in &outline.curves {
            let start = match curve {
                OutlineCurve::Line(a, _)
                | OutlineCurve::Quad(a, _, _)
                | OutlineCurve::Cubic(a, _, _, _) => *a,
            };
            if last != Some(start) {
                if last.is_some() {
                    path.close();
                }
                let (x, y) = p(start);
                path.move_to(x, y);
            }
            match curve {
                OutlineCurve::Line(_, b) => {
                    let (x, y) = p(*b);
                    path.line_to(x, y);
                    last = Some(*b);
                }
                OutlineCurve::Quad(_, c, b) => {
                    let ((cx, cy), (x, y)) = (p(*c), p(*b));
                    path.quad_to(cx, cy, x, y);
                    last = Some(*b);
                }
                OutlineCurve::Cubic(_, c1, c2, b) => {
                    let ((x1, y1), (x2, y2), (x, y)) = (p(*c1), p(*c2), p(*b));
                    path.cubic_to(x1, y1, x2, y2, x, y);
                    last = Some(*b);
                }
            }
        }
        if last.is_some() {
            path.close();
        }
    }
    if let Some(path) = path.finish() {
        pixmap.fill_path(&path, &paint(color), FillRule::Winding, transform, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(255, 0, 0);

    fn white(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]))
    }

    fn plain(shapes: Vec<Shape>) -> Annotation {
        Annotation {
            shapes,
            frame: None,
        }
    }

    fn is_red(img: &RgbaImage, x: u32, y: u32) -> bool {
        let [r, g, b, _] = img.get_pixel(x, y).0;
        r > 200 && g < 60 && b < 60
    }

    #[test]
    fn draws_each_shape_where_expected() {
        let shapes = vec![
            Shape::Rect {
                from: Point::new(10.0, 10.0),
                to: Point::new(90.0, 90.0),
                color: RED,
                width: 4.0,
            },
            Shape::Ellipse {
                from: Point::new(110.0, 10.0),
                to: Point::new(190.0, 90.0),
                color: RED,
                width: 4.0,
            },
            Shape::Arrow {
                from: Point::new(10.0, 150.0),
                to: Point::new(190.0, 150.0),
                color: RED,
                width: 4.0,
            },
        ];
        let out = render(&white(200, 200), &plain(shapes)).unwrap();
        assert!(is_red(&out, 10, 50), "rect edge");
        assert!(!is_red(&out, 50, 50), "rect is hollow");
        assert!(is_red(&out, 150, 10), "ellipse top");
        assert!(!is_red(&out, 150, 50), "ellipse is hollow");
        assert!(is_red(&out, 100, 150), "arrow shaft");
        assert!(is_red(&out, 176, 156), "arrowhead is wider than the shaft");
        assert!(!is_red(&out, 100, 100), "background untouched");
    }

    #[test]
    fn draws_text_inside_its_bounds() {
        let shape = Shape::Text {
            at: Point::new(20.0, 20.0),
            text: "Hi".into(),
            color: RED,
            size: 40.0,
        };
        let b = shape.bounds();
        let out = render(&white(200, 120), &plain(vec![shape])).unwrap();
        let mut red = 0;
        for (x, y, _) in out.enumerate_pixels() {
            if is_red(&out, x, y) {
                red += 1;
                let (fx, fy) = (x as f32, y as f32);
                assert!(fx >= b.left - 1.0 && fx <= b.right + 1.0 && fy >= b.top && fy <= b.bottom);
            }
        }
        assert!(red > 100, "text painted {red} pixels");
    }

    #[test]
    fn frame_pads_rounds_and_offsets_shapes() {
        let frame = Frame::new((200, 100), 7, 0.1);
        let shape = Shape::Rect {
            from: Point::new(0.0, 0.0),
            to: Point::new(50.0, 50.0),
            color: RED,
            width: 4.0,
        };
        let annotation = Annotation {
            shapes: vec![shape],
            frame: Some(frame),
        };
        let out = render(&white(200, 100), &annotation).unwrap();
        let pad = frame.pad();
        assert_eq!(out.dimensions(), (200 + 2 * pad, 100 + 2 * pad));
        assert_eq!(
            out.get_pixel(0, 0).0,
            [0x0f, 0x20, 0x27, 255],
            "gradient start"
        );
        assert_eq!(
            out.get_pixel(pad + 100, pad + 50).0,
            [255; 4],
            "screenshot inside"
        );
        let corner = out.get_pixel(pad + 199, pad).0;
        assert!(corner[0] < 200, "corner is rounded off: {corner:?}");
        assert!(
            is_red(&out, pad + 25, pad),
            "shapes move with the screenshot"
        );
        let flat = Annotation {
            frame: Some(Frame {
                shadow: 0.0,
                ..frame
            }),
            ..annotation
        };
        let flat = render(&white(200, 100), &flat).unwrap();
        let (x, y) = (pad + 100, pad + 100 + 4);
        assert!(
            out.get_pixel(x, y).0[2] < flat.get_pixel(x, y).0[2],
            "shadow darkens below the screenshot"
        );
    }
}
