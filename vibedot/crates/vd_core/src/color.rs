//! Linear-space RGBA color (Godot's `Color`).

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// RGBA color with `f32` channels in **linear** space. Values above 1.0 are
/// allowed (HDR) and are what makes things glow through the bloom pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Pod, Zeroable)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    pub const CYAN: Color = Color::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// From 8-bit sRGB components (what color pickers and hex codes use).
    pub fn from_srgb_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: srgb_to_linear(f32::from(r) / 255.0),
            g: srgb_to_linear(f32::from(g) / 255.0),
            b: srgb_to_linear(f32::from(b) / 255.0),
            a: f32::from(a) / 255.0,
        }
    }

    /// Parses `#rgb`, `#rrggbb` or `#rrggbbaa` (sRGB).
    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.strip_prefix('#').unwrap_or(hex);
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            3 => {
                let nib = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
                Some(Self::from_srgb_u8(nib(0)?, nib(1)?, nib(2)?, 255))
            }
            6 => Some(Self::from_srgb_u8(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Self::from_srgb_u8(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn to_srgb_u8(self) -> [u8; 4] {
        let q = |c: f32| (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), (self.a.clamp(0.0, 1.0) * 255.0).round() as u8]
    }

    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.to_srgb_u8();
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    /// HSV (all components 0..1) to linear color.
    pub fn from_hsv(h: f32, s: f32, v: f32, a: f32) -> Self {
        let h = h.rem_euclid(1.0) * 6.0;
        let i = h.floor();
        let f = h - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i as i32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        // HSV is conventionally defined in sRGB space.
        Self { r: srgb_to_linear(r), g: srgb_to_linear(g), b: srgb_to_linear(b), a }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub fn lerp(self, o: Color, t: f32) -> Self {
        Self {
            r: self.r + (o.r - self.r) * t,
            g: self.g + (o.g - self.g) * t,
            b: self.b + (o.b - self.b) * t,
            a: self.a + (o.a - self.a) * t,
        }
    }

    /// Multiplies RGB by `k` (keeps alpha) - handy for HDR "glow" colors.
    pub fn scaled(self, k: f32) -> Self {
        Self { r: self.r * k, g: self.g * k, b: self.b * k, a: self.a }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn from_array(a: [f32; 4]) -> Self {
        Self { r: a[0], g: a[1], b: a[2], a: a[3] }
    }
}

impl std::ops::Mul for Color {
    type Output = Color;
    fn mul(self, o: Color) -> Color {
        Color { r: self.r * o.r, g: self.g * o.g, b: self.b * o.b, a: self.a * o.a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        for hex in ["#ff8800", "#12345678", "#000000", "#ffffff"] {
            assert_eq!(Color::from_hex(hex).unwrap().to_hex(), hex);
        }
        assert_eq!(Color::from_hex("#fff").unwrap().to_hex(), "#ffffff");
        assert!(Color::from_hex("#zzzzzz").is_none());
    }
}
