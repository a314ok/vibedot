//! # vd_scene
//!
//! The node/scene system (Godot's `scene/main` + `scene/2d`): a hierarchical
//! [`SceneTree`] of typed [`Node`]s backed by a generational arena, compile-time
//! reflection for the inspector and blueprints, and JSON scene files.
#![forbid(unsafe_code)]

pub mod kinds;
pub mod node;
pub mod packed;
pub mod reflect;
pub mod tree;

pub use kinds::*;
pub use node::{KindInfo, Node, NodeId, NodeKind};
pub use packed::{NodeData, SceneFile, SceneIoError};
pub use reflect::{PropHint, PropValue, PropertyInfo, Reflect};
pub use tree::{SceneTree, TreeError};
