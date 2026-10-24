//! The optional frame around a screenshot: a gradient background, padding,
//! rounded corners and a soft drop shadow. The editor previews it with GPUI's
//! own gradient and shadow primitives, so the maths here follows GPUI's
//! shaders and the export looks like the preview.

use serde::{Deserialize, Serialize};

use crate::shapes::{Color, Point};

/// Background presets offered in the editor: name, start and end colour.
pub const GRADIENTS: [(&str, Color, Color); 8] = [
    (
        "Coral",
        Color::rgb(0xf7, 0x6b, 0x55),
        Color::rgb(0xe8, 0x2f, 0x3f),
    ),
    (
        "Mango",
        Color::rgb(0xf7, 0x97, 0x1e),
        Color::rgb(0xff, 0xd2, 0x00),
    ),
    (
        "Peach",
        Color::rgb(0xff, 0x9a, 0x8b),
        Color::rgb(0xff, 0x6a, 0x88),
    ),
    (
        "Grape",
        Color::rgb(0x66, 0x7e, 0xea),
        Color::rgb(0x76, 0x4b, 0xa2),
    ),
    (
        "Lilac",
        Color::rgb(0xa1, 0x8c, 0xd1),
        Color::rgb(0xfb, 0xc2, 0xeb),
    ),
    (
        "Ocean",
        Color::rgb(0x4f, 0xac, 0xfe),
        Color::rgb(0x00, 0xc6, 0xfb),
    ),
    (
        "Mint",
        Color::rgb(0x43, 0xe9, 0x7b),
        Color::rgb(0x38, 0xf9, 0xd7),
    ),
    (
        "Night",
        Color::rgb(0x0f, 0x20, 0x27),
        Color::rgb(0x2c, 0x53, 0x64),
    ),
];

/// Padding presets, as a fraction of the screenshot's longer side.
pub const PADDINGS: [(&str, f32); 3] = [("S", 0.04), ("M", 0.08), ("L", 0.12)];

/// Peak opacity of the drop shadow.
pub const SHADOW_OPACITY: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub from: Color,
    pub to: Color,
    /// Gradient direction in degrees: 0 points up, 90 right, as in CSS.
    pub angle: f32,
    /// Background on each side of the screenshot, in image pixels.
    pub padding: f32,
    /// Corner radius of the screenshot, in image pixels.
    pub radius: f32,
    /// Shadow blur (gaussian sigma) in image pixels; 0 for no shadow.
    pub shadow: f32,
}

impl Frame {
    /// A frame sized for an image of `dims`, with the given gradient preset
    /// and padding fraction.
    pub fn new(dims: (u32, u32), gradient: usize, padding: f32) -> Self {
        let long = dims.0.max(dims.1) as f32;
        let (_, from, to) = GRADIENTS[gradient % GRADIENTS.len()];
        Frame {
            from,
            to,
            angle: 135.0,
            padding: (long * padding).round(),
            radius: (long * 0.006).round().clamp(6.0, 40.0),
            shadow: (long * 0.008).round().clamp(4.0, 40.0),
        }
    }

    /// Padding in whole pixels, so the output size is an integer.
    pub fn pad(&self) -> u32 {
        self.padding.max(0.0).round() as u32
    }

    /// Size of the framed output for an image of `dims`.
    pub fn outer(&self, dims: (u32, u32)) -> (u32, u32) {
        (dims.0 + 2 * self.pad(), dims.1 + 2 * self.pad())
    }

    /// The shadow falls a little below the screenshot.
    pub fn shadow_offset(&self) -> Point {
        Point::new(0.0, self.shadow * 0.5)
    }

    /// Gradient colour at `(x, y)` in a `w`×`h` background, matching GPUI's
    /// linear gradient (sRGB interpolation).
    pub fn color_at(&self, x: f32, y: f32, w: f32, h: f32) -> Color {
        let t = gradient_t(self.angle, x, y, w, h);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color {
            r: mix(self.from.r, self.to.r),
            g: mix(self.from.g, self.to.g),
            b: mix(self.from.b, self.to.b),
            a: mix(self.from.a, self.to.a),
        }
    }

    /// Whether this frame uses gradient preset `index`.
    pub fn uses_gradient(&self, index: usize) -> bool {
        GRADIENTS
            .get(index)
            .is_some_and(|(_, from, to)| self.from == *from && self.to == *to)
    }
}

/// Position along the gradient, 0 to 1. A port of the linear gradient branch
/// of GPUI's fragment shader.
fn gradient_t(angle: f32, x: f32, y: f32, w: f32, h: f32) -> f32 {
    let radians = (angle % 360.0 - 90.0).to_radians();
    let (mut dx, mut dy) = (radians.cos(), radians.sin());
    if w > h {
        dy *= h / w;
    } else {
        dx *= w / h;
    }
    let (cx, cy) = (x - w / 2.0, y - h / 2.0);
    let t = (cx * dx + cy * dy) / (dx * dx + dy * dy).sqrt();
    let t = if dx.abs() > dy.abs() {
        (t + w / 2.0) / w
    } else {
        (t + h / 2.0) / h
    };
    t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_gradient_runs_top_left_to_bottom_right() {
        let frame = Frame::new((100, 100), 0, 0.1);
        assert_eq!(frame.color_at(0.0, 0.0, 100.0, 100.0), frame.from);
        assert_eq!(frame.color_at(100.0, 100.0, 100.0, 100.0), frame.to);
    }

    #[test]
    fn sizes_follow_the_image() {
        let frame = Frame::new((1000, 500), 3, 0.08);
        assert_eq!(frame.pad(), 80);
        assert_eq!(frame.outer((1000, 500)), (1160, 660));
        assert!(frame.uses_gradient(3) && !frame.uses_gradient(0));
    }
}
