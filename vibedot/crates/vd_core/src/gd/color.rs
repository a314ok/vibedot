//! Port of Godot's `core/math/color.{h,cpp}`.
//!
//! Like Godot, components are stored as given (normally sRGB-encoded);
//! [`GdColor::srgb_to_linear`] converts to the engine's linear [`crate::Color`].

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

use super::color_names::NAMED_COLORS;
use super::math_funcs::{self as math, string_num};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GdColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for GdColor {
    /// Godot's `Color()` is opaque black.
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

fn parse_col4(s: &[u8], ofs: usize) -> i32 {
    match s.get(ofs) {
        Some(c @ b'0'..=b'9') => i32::from(c - b'0'),
        Some(c @ b'a'..=b'f') => i32::from(c - b'a') + 10,
        Some(c @ b'A'..=b'F') => i32::from(c - b'A') + 10,
        _ => -1,
    }
}

fn parse_col8(s: &[u8], ofs: usize) -> i32 {
    parse_col4(s, ofs) * 16 + parse_col4(s, ofs + 1)
}

fn append_hex(v: f32, out: &mut String) {
    let v = ((v * 255.0).round() as i32).clamp(0, 255);
    out.push_str(&format!("{v:02x}"));
}

impl GdColor {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn hex(mut h: u32) -> Self {
        let a = (h & 0xFF) as f32 / 255.0;
        h >>= 8;
        let b = (h & 0xFF) as f32 / 255.0;
        h >>= 8;
        let g = (h & 0xFF) as f32 / 255.0;
        h >>= 8;
        let r = (h & 0xFF) as f32 / 255.0;
        Self::new(r, g, b, a)
    }

    pub fn hex64(mut h: u64) -> Self {
        let a = (h & 0xFFFF) as f32 / 65535.0;
        h >>= 16;
        let b = (h & 0xFFFF) as f32 / 65535.0;
        h >>= 16;
        let g = (h & 0xFFFF) as f32 / 65535.0;
        h >>= 16;
        let r = (h & 0xFFFF) as f32 / 65535.0;
        Self::new(r, g, b, a)
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (the `#` is optional).
    /// Invalid input logs an error and returns `Color()`.
    pub fn html(rgba: &str) -> Self {
        if rgba.is_empty() {
            return Self::default();
        }
        let s = rgba.as_bytes();
        let pos = usize::from(s[0] == b'#');
        let digits = s.len() - pos;
        let (r, g, b, a) = match digits {
            3 => (
                parse_col4(s, pos) as f32 / 15.0,
                parse_col4(s, pos + 1) as f32 / 15.0,
                parse_col4(s, pos + 2) as f32 / 15.0,
                1.0,
            ),
            4 => (
                parse_col4(s, pos) as f32 / 15.0,
                parse_col4(s, pos + 1) as f32 / 15.0,
                parse_col4(s, pos + 2) as f32 / 15.0,
                parse_col4(s, pos + 3) as f32 / 15.0,
            ),
            6 => (
                parse_col8(s, pos) as f32 / 255.0,
                parse_col8(s, pos + 2) as f32 / 255.0,
                parse_col8(s, pos + 4) as f32 / 255.0,
                1.0,
            ),
            8 => (
                parse_col8(s, pos) as f32 / 255.0,
                parse_col8(s, pos + 2) as f32 / 255.0,
                parse_col8(s, pos + 4) as f32 / 255.0,
                parse_col8(s, pos + 6) as f32 / 255.0,
            ),
            _ => {
                log::error!("Invalid color code: {rgba}.");
                return Self::default();
            }
        };
        if r < 0.0 || g < 0.0 || b < 0.0 || a < 0.0 {
            log::error!("Invalid color code: {rgba}.");
            return Self::default();
        }
        Self::new(r, g, b, a)
    }

    pub fn html_is_valid(color: &str) -> bool {
        let s = color.as_bytes();
        if s.is_empty() {
            return false;
        }
        let pos = usize::from(s[0] == b'#');
        let digits = s.len() - pos;
        matches!(digits, 3 | 4 | 6 | 8) && s[pos..].iter().all(u8::is_ascii_hexdigit)
    }

    fn find_named(name: &str) -> Option<usize> {
        let norm: String = name
            .chars()
            .filter(|c| !matches!(c, ' ' | '-' | '_' | '\'' | '.'))
            .flat_map(char::to_uppercase)
            .collect();
        NAMED_COLORS.iter().position(|(n, _)| n.replace('_', "") == norm)
    }

    /// Godot's named colors (`"red"`, `"Slate Blue"`, `"white_smoke"`...).
    pub fn named(name: &str) -> Self {
        match Self::find_named(name) {
            Some(i) => Self::hex(NAMED_COLORS[i].1),
            None => {
                log::error!("Invalid color name: {name}.");
                Self::default()
            }
        }
    }

    pub fn named_or(name: &str, default: GdColor) -> Self {
        Self::find_named(name).map_or(default, |i| Self::hex(NAMED_COLORS[i].1))
    }

    /// `Color.from_string()`: html code or named color.
    pub fn from_string(s: &str, default: GdColor) -> Self {
        if Self::html_is_valid(s) { Self::html(s) } else { Self::named_or(s, default) }
    }

    pub fn from_hsv(h: f32, s: f32, v: f32, alpha: f32) -> Self {
        let mut c = Self::default();
        c.set_hsv(h, s, v, alpha);
        c
    }

    pub fn from_rgba8(r: i64, g: i64, b: i64, a: i64) -> Self {
        Self::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0)
    }

    pub fn set_hsv(&mut self, h: f32, s: f32, v: f32, alpha: f32) {
        self.a = alpha;
        if s == 0.0 {
            // Achromatic (gray).
            self.r = v;
            self.g = v;
            self.b = v;
            return;
        }
        let h = (h * 6.0) % 6.0;
        let i = h.floor() as i32;
        let f = h - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        (self.r, self.g, self.b) = match i {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
    }

    pub fn get_h(&self) -> f32 {
        let min = self.r.min(self.g).min(self.b);
        let max = self.r.max(self.g).max(self.b);
        let delta = max - min;
        if delta == 0.0 {
            return 0.0;
        }
        let mut h = if self.r == max {
            (self.g - self.b) / delta
        } else if self.g == max {
            2.0 + (self.b - self.r) / delta
        } else {
            4.0 + (self.r - self.g) / delta
        };
        h /= 6.0;
        if h < 0.0 {
            h += 1.0;
        }
        h
    }

    pub fn get_s(&self) -> f32 {
        let min = self.r.min(self.g).min(self.b);
        let max = self.r.max(self.g).max(self.b);
        if max != 0.0 { (max - min) / max } else { 0.0 }
    }

    pub fn get_v(&self) -> f32 {
        self.r.max(self.g).max(self.b)
    }

    pub fn get_luminance(&self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    pub fn to_html(&self, alpha: bool) -> String {
        let mut s = String::with_capacity(8);
        append_hex(self.r, &mut s);
        append_hex(self.g, &mut s);
        append_hex(self.b, &mut s);
        if alpha {
            append_hex(self.a, &mut s);
        }
        s
    }

    fn q8(v: f32) -> u32 {
        u32::from((v * 255.0).round() as u8)
    }

    fn q16(v: f32) -> u64 {
        u64::from((v * 65535.0).round() as u16)
    }

    pub fn to_argb32(&self) -> u32 {
        (Self::q8(self.a) << 24) | (Self::q8(self.r) << 16) | (Self::q8(self.g) << 8) | Self::q8(self.b)
    }

    pub fn to_abgr32(&self) -> u32 {
        (Self::q8(self.a) << 24) | (Self::q8(self.b) << 16) | (Self::q8(self.g) << 8) | Self::q8(self.r)
    }

    pub fn to_rgba32(&self) -> u32 {
        (Self::q8(self.r) << 24) | (Self::q8(self.g) << 16) | (Self::q8(self.b) << 8) | Self::q8(self.a)
    }

    pub fn to_argb64(&self) -> u64 {
        (Self::q16(self.a) << 48) | (Self::q16(self.r) << 32) | (Self::q16(self.g) << 16) | Self::q16(self.b)
    }

    pub fn to_abgr64(&self) -> u64 {
        (Self::q16(self.a) << 48) | (Self::q16(self.b) << 32) | (Self::q16(self.g) << 16) | Self::q16(self.r)
    }

    pub fn to_rgba64(&self) -> u64 {
        (Self::q16(self.r) << 48) | (Self::q16(self.g) << 32) | (Self::q16(self.b) << 16) | Self::q16(self.a)
    }

    pub fn inverted(&self) -> Self {
        Self::new(1.0 - self.r, 1.0 - self.g, 1.0 - self.b, self.a)
    }

    pub fn lerp(&self, to: GdColor, w: f32) -> Self {
        Self::new(
            math::lerp(self.r, to.r, w),
            math::lerp(self.g, to.g, w),
            math::lerp(self.b, to.b, w),
            math::lerp(self.a, to.a, w),
        )
    }

    pub fn darkened(&self, amount: f32) -> Self {
        Self::new(self.r * (1.0 - amount), self.g * (1.0 - amount), self.b * (1.0 - amount), self.a)
    }

    pub fn lightened(&self, amount: f32) -> Self {
        Self::new(
            self.r + (1.0 - self.r) * amount,
            self.g + (1.0 - self.g) * amount,
            self.b + (1.0 - self.b) * amount,
            self.a,
        )
    }

    pub fn blend(&self, over: GdColor) -> Self {
        let sa = 1.0 - over.a;
        let a = self.a * sa + over.a;
        if a == 0.0 {
            return Self::new(0.0, 0.0, 0.0, 0.0);
        }
        Self::new(
            (self.r * self.a * sa + over.r * over.a) / a,
            (self.g * self.a * sa + over.g * over.a) / a,
            (self.b * self.a * sa + over.b * over.a) / a,
            a,
        )
    }

    pub fn clamp(&self, min: GdColor, max: GdColor) -> Self {
        Self::new(
            self.r.clamp(min.r, max.r),
            self.g.clamp(min.g, max.g),
            self.b.clamp(min.b, max.b),
            self.a.clamp(min.a, max.a),
        )
    }

    pub fn srgb_to_linear(&self) -> Self {
        let f = |c: f32| {
            if c < 0.04045 { c * (1.0 / 12.92) } else { (((f64::from(c) + 0.055) * (1.0 / (1.0 + 0.055))) as f32).powf(2.4) }
        };
        Self::new(f(self.r), f(self.g), f(self.b), self.a)
    }

    pub fn linear_to_srgb(&self) -> Self {
        let f = |c: f32| {
            if c < 0.003_130_8 { 12.92 * c } else { ((1.0 + 0.055) * f64::from(c.powf(1.0 / 2.4)) - 0.055) as f32 }
        };
        Self::new(f(self.r), f(self.g), f(self.b), self.a)
    }

    pub fn is_equal_approx(&self, o: &GdColor) -> bool {
        math::is_equal_approx(self.r, o.r)
            && math::is_equal_approx(self.g, o.g)
            && math::is_equal_approx(self.b, o.b)
            && math::is_equal_approx(self.a, o.a)
    }

    /// Converts this (sRGB-encoded) Godot color to the engine's linear color.
    pub fn to_engine(&self) -> crate::Color {
        let l = self.srgb_to_linear();
        crate::Color::rgba(l.r, l.g, l.b, l.a)
    }

    pub fn from_engine(c: crate::Color) -> Self {
        GdColor::new(c.r, c.g, c.b, c.a).linear_to_srgb()
    }
}

impl fmt::Display for GdColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = |v: f32| string_num(f64::from(v), 4);
        write!(f, "({}, {}, {}, {})", n(self.r), n(self.g), n(self.b), n(self.a))
    }
}

macro_rules! color_ops {
    ($Tr:ident $f:ident $op:tt) => {
        impl $Tr for GdColor {
            type Output = GdColor;
            fn $f(self, o: GdColor) -> GdColor {
                GdColor::new(self.r $op o.r, self.g $op o.g, self.b $op o.b, self.a $op o.a)
            }
        }
        impl $Tr<f32> for GdColor {
            type Output = GdColor;
            fn $f(self, s: f32) -> GdColor {
                GdColor::new(self.r $op s, self.g $op s, self.b $op s, self.a $op s)
            }
        }
    };
}
color_ops!(Add add +);
color_ops!(Sub sub -);
color_ops!(Mul mul *);
color_ops!(Div div /);

impl Neg for GdColor {
    type Output = GdColor;
    /// Godot's unary minus inverts all four components (`1 - c`).
    fn neg(self) -> GdColor {
        GdColor::new(1.0 - self.r, 1.0 - self.g, 1.0 - self.b, 1.0 - self.a)
    }
}
