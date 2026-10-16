//! Minimal compile-time reflection, replacing Godot's `ClassDB` property
//! registration (`ADD_PROPERTY`, `_bind_methods`).
//!
//! The [`node_kind!`] macro declares a struct *once* and derives its
//! `Default`, serde support and the [`Reflect`] implementation from that single
//! definition, so the inspector, scene files and blueprints never drift apart.

use std::sync::Arc;

use vd_core::{Color, VType, Variant, Vec2};

/// Editor hint for a property (Godot's `PropertyHint`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropHint {
    None,
    /// Numeric slider range.
    Range {
        min: f32,
        max: f32,
        step: f32,
    },
    /// Angle stored in radians, edited in degrees.
    Angle,
    /// One of a fixed list of strings.
    Enum(&'static [&'static str]),
    /// Path to a resource with one of the given extensions.
    File(&'static [&'static str]),
    Multiline,
    /// Color that may exceed 1.0 (glow).
    HdrColor,
}

#[derive(Clone, Copy, Debug)]
pub struct PropertyInfo {
    pub name: &'static str,
    pub ty: VType,
    pub hint: PropHint,
    /// Inspector section title.
    pub group: &'static str,
}

/// Conversion between a Rust field type and [`Variant`].
pub trait PropValue: Sized {
    const VTYPE: VType;
    fn to_variant(&self) -> Variant;
    fn from_variant(v: &Variant) -> Self;
}

impl PropValue for f32 {
    const VTYPE: VType = VType::Number;
    fn to_variant(&self) -> Variant {
        Variant::Number(f64::from(*self))
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_f32()
    }
}

impl PropValue for i32 {
    const VTYPE: VType = VType::Number;
    fn to_variant(&self) -> Variant {
        Variant::Number(f64::from(*self))
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_number().round() as i32
    }
}

impl PropValue for u32 {
    const VTYPE: VType = VType::Number;
    fn to_variant(&self) -> Variant {
        Variant::Number(f64::from(*self))
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_number().round().max(0.0) as u32
    }
}

impl PropValue for bool {
    const VTYPE: VType = VType::Bool;
    fn to_variant(&self) -> Variant {
        Variant::Bool(*self)
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_bool()
    }
}

impl PropValue for Vec2 {
    const VTYPE: VType = VType::Vec2;
    fn to_variant(&self) -> Variant {
        Variant::Vec2(*self)
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_vec2()
    }
}

impl PropValue for Color {
    const VTYPE: VType = VType::Color;
    fn to_variant(&self) -> Variant {
        Variant::Color(*self)
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_color()
    }
}

impl PropValue for String {
    const VTYPE: VType = VType::String;
    fn to_variant(&self) -> Variant {
        Variant::String(Arc::from(self.as_str()))
    }
    fn from_variant(v: &Variant) -> Self {
        v.to_string()
    }
}

/// Runtime property access.
pub trait Reflect {
    fn properties(&self, out: &mut Vec<PropertyInfo>);
    fn get_prop(&self, name: &str) -> Option<Variant>;
    /// Returns `false` if the property does not exist.
    fn set_prop(&mut self, name: &str, value: &Variant) -> bool;
}

/// Declares a string-backed enum usable as a property (`PropHint::Enum`).
#[macro_export]
macro_rules! prop_enum {
    ($(#[$m:meta])* $vis:vis enum $name:ident { $($variant:ident),+ $(,)? } default $def:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        $vis enum $name { $($variant),+ }

        impl Default for $name {
            fn default() -> Self { $name::$def }
        }

        impl $name {
            pub const NAMES: &'static [&'static str] = &[$(stringify!($variant)),+];
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => stringify!($variant)),+ }
            }
            pub fn parse(s: &str) -> Option<Self> {
                match s { $(stringify!($variant) => Some($name::$variant),)+ _ => None }
            }
        }

        impl $crate::reflect::PropValue for $name {
            const VTYPE: vd_core::VType = vd_core::VType::String;
            fn to_variant(&self) -> vd_core::Variant { vd_core::Variant::str(self.as_str()) }
            fn from_variant(v: &vd_core::Variant) -> Self {
                match v {
                    vd_core::Variant::Number(n) => Self::ALL.get(*n as usize).copied().unwrap_or_default(),
                    other => Self::parse(&other.to_string()).unwrap_or_default(),
                }
            }
        }
    };
}

/// Declares a reflected node-kind struct.
///
/// ```ignore
/// node_kind! {
///     /// Docs
///     pub struct Sprite2D ["Sprite"] {
///         size: Vec2 = vec2(64.0, 64.0) => PropHint::None,
///     }
/// }
/// ```
#[macro_export]
macro_rules! node_kind {
    (
        $(#[$m:meta])*
        pub struct $name:ident [$group:literal] {
            $( $(#[$fm:meta])* $field:ident : $fty:ty = $default:expr => $hint:expr ),* $(,)?
        }
        $( runtime { $( $(#[$rm:meta])* $rfield:ident : $rty:ty ),* $(,)? } )?
    ) => {
        $(#[$m])*
        #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
        #[serde(default)]
        pub struct $name {
            $( $(#[$fm])* pub $field: $fty, )*
            $( $( $(#[$rm])* #[serde(skip)] pub $rfield: $rty, )* )?
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $( $field: $default, )*
                    $( $( $rfield: Default::default(), )* )?
                }
            }
        }

        impl $crate::reflect::Reflect for $name {
            #[allow(unused_variables)]
            fn properties(&self, out: &mut Vec<$crate::reflect::PropertyInfo>) {
                $(
                    out.push($crate::reflect::PropertyInfo {
                        name: stringify!($field),
                        ty: <$fty as $crate::reflect::PropValue>::VTYPE,
                        hint: $hint,
                        group: $group,
                    });
                )*
            }
            fn get_prop(&self, name: &str) -> Option<vd_core::Variant> {
                match name {
                    $( stringify!($field) => Some($crate::reflect::PropValue::to_variant(&self.$field)), )*
                    _ => None,
                }
            }
            #[allow(unused_variables)]
            fn set_prop(&mut self, name: &str, value: &vd_core::Variant) -> bool {
                match name {
                    $( stringify!($field) => {
                        self.$field = <$fty as $crate::reflect::PropValue>::from_variant(value);
                        true
                    } )*
                    _ => false,
                }
            }
        }
    };
}
