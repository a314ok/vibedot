//! # vd_core
//!
//! Foundation types for the Vibedot engine - the Rust counterpart of Godot's
//! `core/` directory: math, colors, interned names, the dynamic [`Variant`]
//! and the generational [`Arena`] that replaces Godot's `ObjectDB`.
//!
//! This crate contains no `unsafe` code.
#![forbid(unsafe_code)]

pub mod arena;
pub mod color;
pub mod math;
pub mod rng;
pub mod string_name;
pub mod variant;

pub use arena::{Arena, Handle};
pub use color::Color;
pub use math::{Affine2, Mat2, Mat3, Mat4, Rect2, Transform2D, Vec2, Vec3, Vec4, vec2, vec3, vec4};
pub use rng::Rng;
pub use string_name::StringName;
pub use variant::{ObjectId, VType, Variant};

/// Engine version string, shown in the editor and embedded in exported games.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
