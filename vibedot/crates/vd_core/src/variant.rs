//! `Variant`: the dynamic value type shared by the inspector, scene files,
//! node properties and the blueprint virtual machine.
//!
//! Godot's Variant has ~40 types. We deliberately keep a small, orthogonal set
//! that covers what designers need in visual scripting. Everything is cheap to
//! clone: strings and arrays are reference counted (`Arc`), the rest is `Copy`.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::math::Vec2;

/// Opaque object identity (Godot's `ObjectID`). The scene crate encodes node
/// handles into it; a stale id simply resolves to "no node".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(pub u64);

/// Static type of a value, used for blueprint pins and property metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VType {
    #[default]
    Any,
    Bool,
    Number,
    Vec2,
    Color,
    String,
    Object,
    Array,
}

impl VType {
    pub const ALL: [VType; 8] =
        [VType::Any, VType::Bool, VType::Number, VType::Vec2, VType::Color, VType::String, VType::Object, VType::Array];

    pub fn name(self) -> &'static str {
        match self {
            VType::Any => "Any",
            VType::Bool => "Bool",
            VType::Number => "Number",
            VType::Vec2 => "Vector2",
            VType::Color => "Color",
            VType::String => "String",
            VType::Object => "Node",
            VType::Array => "Array",
        }
    }

    /// Can a value of type `from` be fed into a pin of type `self`?
    /// Mirrors the implicit conversions performed by [`Variant`] accessors.
    pub fn accepts(self, from: VType) -> bool {
        use VType::*;
        match (self, from) {
            (Any, _) | (_, Any) => true,
            (a, b) if a == b => true,
            (String, _) => true, // everything stringifies
            (Number, Bool) | (Bool, Number) => true,
            (Bool, Object) => true, // "is valid"
            _ => false,
        }
    }

    pub fn default_value(self) -> Variant {
        match self {
            VType::Any | VType::Object => Variant::Nil,
            VType::Bool => Variant::Bool(false),
            VType::Number => Variant::Number(0.0),
            VType::Vec2 => Variant::Vec2(Vec2::ZERO),
            VType::Color => Variant::Color(Color::WHITE),
            VType::String => Variant::String(Arc::from("")),
            VType::Array => Variant::Array(Arc::new(Vec::new())),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    #[default]
    Nil,
    Bool(bool),
    Number(f64),
    Vec2(Vec2),
    Color(Color),
    String(Arc<str>),
    Object(ObjectId),
    Array(Arc<Vec<Variant>>),
}

impl Variant {
    pub fn vtype(&self) -> VType {
        match self {
            Variant::Nil => VType::Any,
            Variant::Bool(_) => VType::Bool,
            Variant::Number(_) => VType::Number,
            Variant::Vec2(_) => VType::Vec2,
            Variant::Color(_) => VType::Color,
            Variant::String(_) => VType::String,
            Variant::Object(_) => VType::Object,
            Variant::Array(_) => VType::Array,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Variant::Nil)
    }

    /// Truthiness, following Godot rules (empty / zero / nil are false).
    pub fn to_bool(&self) -> bool {
        match self {
            Variant::Nil => false,
            Variant::Bool(b) => *b,
            Variant::Number(n) => *n != 0.0,
            Variant::Vec2(v) => *v != Vec2::ZERO,
            Variant::Color(_) => true,
            Variant::String(s) => !s.is_empty(),
            Variant::Object(_) => true,
            Variant::Array(a) => !a.is_empty(),
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Variant::Bool(b) => f64::from(u8::from(*b)),
            Variant::Number(n) => *n,
            Variant::Vec2(v) => f64::from(v.length()),
            Variant::String(s) => s.trim().parse().unwrap_or(0.0),
            Variant::Array(a) => a.len() as f64,
            _ => 0.0,
        }
    }

    #[inline]
    pub fn to_f32(&self) -> f32 {
        self.to_number() as f32
    }

    pub fn to_vec2(&self) -> Vec2 {
        match self {
            Variant::Vec2(v) => *v,
            Variant::Number(n) => Vec2::splat(*n as f32),
            _ => Vec2::ZERO,
        }
    }

    pub fn to_color(&self) -> Color {
        match self {
            Variant::Color(c) => *c,
            Variant::String(s) => Color::from_hex(s).unwrap_or(Color::WHITE),
            Variant::Number(n) => Color::rgb(*n as f32, *n as f32, *n as f32),
            _ => Color::WHITE,
        }
    }

    pub fn to_object(&self) -> Option<ObjectId> {
        match self {
            Variant::Object(o) => Some(*o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Variant] {
        match self {
            Variant::Array(a) => a,
            _ => &[],
        }
    }

    /// Converts the value to `ty`, applying the implicit conversions.
    pub fn coerce(&self, ty: VType) -> Variant {
        match ty {
            VType::Any => self.clone(),
            VType::Bool => Variant::Bool(self.to_bool()),
            VType::Number => Variant::Number(self.to_number()),
            VType::Vec2 => Variant::Vec2(self.to_vec2()),
            VType::Color => Variant::Color(self.to_color()),
            VType::String => Variant::String(Arc::from(self.to_string())),
            VType::Object => match self {
                Variant::Object(_) => self.clone(),
                _ => Variant::Nil,
            },
            VType::Array => match self {
                Variant::Array(_) => self.clone(),
                Variant::Nil => Variant::Array(Arc::new(Vec::new())),
                other => Variant::Array(Arc::new(vec![other.clone()])),
            },
        }
    }

    pub fn str(s: impl AsRef<str>) -> Variant {
        Variant::String(Arc::from(s.as_ref()))
    }

    /// Loose equality used by the `==` blueprint node.
    pub fn loose_eq(&self, other: &Variant) -> bool {
        match (self, other) {
            (Variant::Number(a), Variant::Bool(_)) | (Variant::Bool(_), Variant::Number(a)) => {
                (*a != 0.0) == (self.to_bool() && other.to_bool())
            }
            (Variant::Number(a), Variant::Number(b)) => (a - b).abs() < 1e-9,
            (Variant::Vec2(a), Variant::Vec2(b)) => a.abs_diff_eq(*b, 1e-5),
            (Variant::String(a), b) | (b, Variant::String(a)) if !matches!(b, Variant::String(_)) => {
                **a == *b.to_string()
            }
            _ => self == other,
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Variant::Nil => f.write_str("null"),
            Variant::Bool(b) => write!(f, "{b}"),
            Variant::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", (n * 1000.0).round() / 1000.0)
                }
            }
            Variant::Vec2(v) => write!(f, "({:.2}, {:.2})", v.x, v.y),
            Variant::Color(c) => f.write_str(&c.to_hex()),
            Variant::String(s) => f.write_str(s),
            Variant::Object(o) => write!(f, "<Node#{:x}>", o.0),
            Variant::Array(a) => {
                f.write_str("[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
        }
    }
}

macro_rules! impl_from {
    ($t:ty, $v:ident, $conv:expr) => {
        impl From<$t> for Variant {
            fn from(x: $t) -> Self {
                #[allow(clippy::redundant_closure_call)]
                Variant::$v(($conv)(x))
            }
        }
    };
}
impl_from!(bool, Bool, |x| x);
impl_from!(f64, Number, |x| x);
impl_from!(f32, Number, f64::from);
impl_from!(i32, Number, f64::from);
impl_from!(Vec2, Vec2, |x| x);
impl_from!(Color, Color, |x| x);
impl_from!(&str, String, Arc::from);
impl_from!(String, String, Arc::from);
impl_from!(ObjectId, Object, |x| x);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coercions() {
        assert_eq!(Variant::from("42").to_number(), 42.0);
        assert!(Variant::from(1.0).to_bool());
        assert!(!Variant::Nil.to_bool());
        assert_eq!(Variant::from(3.0).to_string(), "3");
        assert_eq!(Variant::from(0.12345).to_string(), "0.123");
        assert_eq!(Variant::from(2.0).coerce(VType::Vec2), Variant::Vec2(Vec2::splat(2.0)));
        assert!(Variant::from("5").loose_eq(&Variant::from(5.0)));
    }

    #[test]
    fn type_compat() {
        assert!(VType::String.accepts(VType::Number));
        assert!(VType::Number.accepts(VType::Bool));
        assert!(!VType::Vec2.accepts(VType::Color));
        assert!(VType::Any.accepts(VType::Object));
    }

    #[test]
    fn serde() {
        let v = Variant::Array(Arc::new(vec![Variant::from(1.0), Variant::from("hi")]));
        let s = serde_json::to_string(&v).unwrap();
        let back: Variant = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}
