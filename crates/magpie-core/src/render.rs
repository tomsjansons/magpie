//! Draws annotations onto a screenshot with tiny-skia. This is the export
//! path: what gets saved and copied never depends on the editor's renderer.

use std::io::Cursor;

use ab_glyph::{Font, OutlineCurve};
use anyhow::{Context, Result};
use image::{ImageFormat, RgbaImage};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::shapes::{Color, Point, Shape, arrow_geometry, font, layout_text};

/// `base` with `shapes` painted on top, in order.
pub fn render(base: &RgbaImage, shapes: &[Shape]) -> Result<RgbaImage> {
    let (w, h) = base.dimensions();
    let mut pixmap = Pixmap::new(w, h).context("image has no pixels")?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(base.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }

    for shape in shapes {
        draw(&mut pixmap, shape);
    }

    let mut out = RgbaImage::new(w, h);
    for (dst, src) in out.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    Ok(out)
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

fn draw(pixmap: &mut Pixmap, shape: &Shape) {
    let id = Transform::identity();
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
        } => draw_text(pixmap, *at, text, *color, *size),
    }
}

/// Fills each glyph outline as a path, so text antialiases like the shapes.
fn draw_text(pixmap: &mut Pixmap, at: Point, text: &str, color: Color, size: f32) {
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
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(255, 0, 0);

    fn white(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]))
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
        let out = render(&white(200, 200), &shapes).unwrap();
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
        let out = render(&white(200, 120), &[shape]).unwrap();
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
}
