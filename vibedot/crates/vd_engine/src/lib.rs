//! # vd_engine
//!
//! The Vibedot runtime: project settings, input map, asset packs, the game
//! loop that drives the scene tree, blueprints, native Rust scripts and
//! physics, and the scene → draw-list conversion used by both the player
//! and the editor.
#![forbid(unsafe_code)]

pub mod assets;
pub mod draw;
pub mod game;
pub mod input;
pub mod project;
pub mod script;
pub mod templates;

pub use assets::{AssetSource, DirAssets, MemAssets, PackAssets, PackError, crc32, pack_source, write_pack};
pub use draw::{draw_collision_shapes, draw_tree, env_settings};
pub use game::{Game, HostState, LogEntry, LogLevel};
pub use input::{InputMap, InputState};
pub use project::{PROJECT_FILE, ProjectSettings, default_input_map};
pub use script::{NodeScript, ScriptCtx, ScriptRegistry};

#[cfg(test)]
mod tests;
