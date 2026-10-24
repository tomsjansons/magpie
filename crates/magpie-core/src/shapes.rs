//! Annotation shapes and the geometry both the editor and the renderer use,
//! so what the editor draws and what gets exported agree.
//!
//! All coordinates are image pixels, origin top-left.

use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The one font used for text annotations, on screen and in exports.
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/Roboto-Bold.ttf");
pub const FONT_FAMILY: &str = "Roboto";

/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.25;

pub fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("bundled font parses"))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }

    fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// An sRGB color, serialized as `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if self.a != 255 {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let byte = |i: usize| {
            hex.get(i..i + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("invalid color {s:?}"))
        };
        match hex.len() {
            6 => Ok(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Ok(Color {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: byte(6)?,
            }),
            _ => Err(format!("invalid color {s:?}")),
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Axis-aligned box from two opposite corners in any order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Bounds {
    pub fn from_corners(a: Point, b: Point) -> Self {
        Bounds {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    fn contains(&self, p: Point, slack: f32) -> bool {
        p.x >= self.left - slack
            && p.x <= self.right + slack
            && p.y >= self.top - slack
            && p.y <= self.bottom + slack
    }

    fn grow(self, by: f32) -> Self {
        Bounds {
            left: self.left - by,
            top: self.top - by,
            right: self.right + by,
            bottom: self.bottom + by,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Arrow {
        from: Point,
        to: Point,
        color: Color,
        width: f32,
    },
    Rect {
        from: Point,
        to: Point,
        color: Color,
        width: f32,
    },
    Ellipse {
        from: Point,
        to: Point,
        color: Color,
        width: f32,
    },
    /// `at` is the top-left of the first line.
    Text {
        at: Point,
        text: String,
        color: Color,
        size: f32,
    },
}

impl Shape {
    pub fn color(&self) -> Color {
        match self {
            Shape::Arrow { color, .. }
            | Shape::Rect { color, .. }
            | Shape::Ellipse { color, .. }
            | Shape::Text { color, .. } => *color,
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        let shift = |p: &mut Point| {
            p.x += dx;
            p.y += dy;
        };
        match self {
            Shape::Arrow { from, to, .. }
            | Shape::Rect { from, to, .. }
            | Shape::Ellipse { from, to, .. } => {
                shift(from);
                shift(to);
            }
            Shape::Text { at, .. } => shift(at),
        }
    }

    /// The area the shape covers, including stroke width and arrowhead.
    pub fn bounds(&self) -> Bounds {
        match self {
            Shape::Arrow {
                from, to, width, ..
            } => Bounds::from_corners(*from, *to).grow(arrow_head_length(*width)),
            Shape::Rect {
                from, to, width, ..
            }
            | Shape::Ellipse {
                from, to, width, ..
            } => Bounds::from_corners(*from, *to).grow(width / 2.0),
            Shape::Text { at, text, size, .. } => {
                let layout = layout_text(text, *size);
                Bounds {
                    left: at.x,
                    top: at.y,
                    right: at.x + layout.width,
                    bottom: at.y + layout.height,
                }
            }
        }
    }

    /// Whether `p` lands on the shape: on the line or outline for strokes
    /// (outlines are hollow, so clicks inside a box reach what is beneath),
    /// anywhere in the text box for text.
    pub fn hit(&self, p: Point, tolerance: f32) -> bool {
        match self {
            Shape::Arrow {
                from, to, width, ..
            } => segment_distance(p, *from, *to) <= width / 2.0 + tolerance,
            Shape::Rect {
                from, to, width, ..
            } => {
                let b = Bounds::from_corners(*from, *to);
                let slack = width / 2.0 + tolerance;
                let inside = Bounds {
                    left: b.left + slack,
                    top: b.top + slack,
                    right: b.right - slack,
                    bottom: b.bottom - slack,
                };
                b.contains(p, slack)
                    && !(inside.width() > 0.0 && inside.height() > 0.0 && inside.contains(p, 0.0))
            }
            Shape::Ellipse {
                from, to, width, ..
            } => {
                let b = Bounds::from_corners(*from, *to);
                let (rx, ry) = (b.width() / 2.0, b.height() / 2.0);
                if rx < 1.0 || ry < 1.0 {
                    return b.contains(p, width / 2.0 + tolerance);
                }
                let (cx, cy) = (b.left + rx, b.top + ry);
                let r = ((p.x - cx) / rx).hypot((p.y - cy) / ry);
                (r - 1.0).abs() * rx.min(ry) <= width / 2.0 + tolerance
            }
            Shape::Text { .. } => self.bounds().contains(p, tolerance),
        }
    }
}

/// How long the arrowhead is for a given stroke width.
pub fn arrow_head_length(width: f32) -> f32 {
    (width * 4.0).max(14.0)
}

/// The arrowhead triangle (tip first) and the point where the shaft should
/// stop so its end hides under the head.
pub fn arrow_geometry(from: Point, to: Point, width: f32) -> ([Point; 3], Point) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let len = dx.hypot(dy).max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    // Never longer than the arrow itself, so short arrows stay arrows.
    let head = arrow_head_length(width).min(len);
    let half = head * 0.55;
    let base = Point::new(to.x - ux * head, to.y - uy * head);
    let left = Point::new(base.x - uy * half, base.y + ux * half);
    let right = Point::new(base.x + uy * half, base.y - ux * half);
    let shaft_end = Point::new(to.x - ux * head * 0.8, to.y - uy * head * 0.8);
    ([to, left, right], shaft_end)
}

fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

/// A glyph placed by [`layout_text`]: pen position on the baseline, relative
/// to the text's top-left.
pub struct PlacedGlyph {
    pub id: GlyphId,
    pub x: f32,
    pub baseline: f32,
}

pub struct TextLayout {
    pub glyphs: Vec<PlacedGlyph>,
    pub width: f32,
    pub height: f32,
    /// Font units to pixels.
    pub scale: f32,
}

/// Lays out `text` at `size` px per em with CSS-style line boxes: each line is
/// `size * LINE_HEIGHT` tall with the glyphs centred in it, which is what GPUI
/// does with the same line height.
pub fn layout_text(text: &str, size: f32) -> TextLayout {
    let font = font();
    let scale = size / font.units_per_em().unwrap_or(1000.0);
    let ascent = font.ascent_unscaled() * scale;
    let descent = font.descent_unscaled() * scale; // negative
    let line_height = size * LINE_HEIGHT;
    let half_leading = (line_height - (ascent - descent)) / 2.0;

    let mut glyphs = Vec::new();
    let mut width: f32 = 0.0;
    let lines: Vec<&str> = text.split('\n').collect();
    for (row, line) in lines.iter().enumerate() {
        let baseline = row as f32 * line_height + half_leading + ascent;
        let mut x = 0.0;
        let mut previous = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = previous {
                x += font.kern_unscaled(prev, id) * scale;
            }
            glyphs.push(PlacedGlyph { id, x, baseline });
            x += font.h_advance_unscaled(id) * scale;
            previous = Some(id);
        }
        width = width.max(x);
    }
    TextLayout {
        glyphs,
        width,
        height: lines.len() as f32 * line_height,
        scale,
    }
}

/// `<version>.json`: everything drawn on a variant, in paint order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub shapes: Vec<Shape>,
    /// Gradient background around the screenshot, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<crate::frame::Frame>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_round_trips() {
        let c: Color = "#ff3b30".parse().unwrap();
        assert_eq!(c, Color::rgb(0xff, 0x3b, 0x30));
        assert_eq!(c.to_string(), "#ff3b30");
        let translucent: Color = "#00000080".parse().unwrap();
        assert_eq!(translucent.to_string(), "#00000080");
        assert!("#12345".parse::<Color>().is_err());
    }

    #[test]
    fn shapes_serialize_with_kind_tag() {
        let shape = Shape::Rect {
            from: Point::new(1.0, 2.0),
            to: Point::new(3.0, 4.0),
            color: Color::rgb(255, 0, 0),
            width: 4.0,
        };
        let json = serde_json::to_string(&shape).unwrap();
        assert!(json.contains(r#""kind":"rect""#), "{json}");
        assert_eq!(serde_json::from_str::<Shape>(&json).unwrap(), shape);
    }

    #[test]
    fn rect_hits_outline_not_inside() {
        let rect = Shape::Rect {
            from: Point::new(0.0, 0.0),
            to: Point::new(100.0, 100.0),
            color: Color::rgb(0, 0, 0),
            width: 4.0,
        };
        assert!(rect.hit(Point::new(0.0, 50.0), 3.0));
        assert!(!rect.hit(Point::new(50.0, 50.0), 3.0));
        assert!(!rect.hit(Point::new(150.0, 50.0), 3.0));
    }

    #[test]
    fn ellipse_and_arrow_hits() {
        let ellipse = Shape::Ellipse {
            from: Point::new(0.0, 0.0),
            to: Point::new(200.0, 100.0),
            color: Color::rgb(0, 0, 0),
            width: 4.0,
        };
        assert!(ellipse.hit(Point::new(100.0, 0.0), 3.0));
        assert!(!ellipse.hit(Point::new(100.0, 50.0), 3.0));
        let arrow = Shape::Arrow {
            from: Point::new(0.0, 0.0),
            to: Point::new(100.0, 0.0),
            color: Color::rgb(0, 0, 0),
            width: 4.0,
        };
        assert!(arrow.hit(Point::new(50.0, 3.0), 3.0));
        assert!(!arrow.hit(Point::new(50.0, 20.0), 3.0));
    }

    #[test]
    fn text_layout_grows_with_lines() {
        let one = layout_text("Hello", 20.0);
        let two = layout_text("Hello\nworld!", 20.0);
        assert!(one.width > 20.0);
        assert_eq!(one.height, 25.0);
        assert_eq!(two.height, 50.0);
        assert!(two.glyphs.last().unwrap().baseline > one.glyphs[0].baseline);
    }
}
