//! Godot-compatible math API (`Vector2`, `Rect2`, `Transform2D`, `Color`,
//! `Math::*`), ported from `godot/core/math` with the same method names,
//! argument order and edge-case behaviour. Godot's own unit tests for these
//! types are ported in `tests.rs`.
//!
//! The engine internally uses SIMD-friendly `glam` types; every type here
//! converts to/from them losslessly (`From` impls, `to_affine`, `to_engine`).

pub mod color;
pub mod color_names;
pub mod math_funcs;
pub mod rect2;
pub mod transform2d;
pub mod vector2;

pub use color::GdColor;
pub use math_funcs as math;
pub use rect2::{Rect2, Side};
pub use transform2d::Transform2D;
pub use vector2::{Axis, Point2, Size2, Vector2};

#[cfg(test)]
mod tests;
