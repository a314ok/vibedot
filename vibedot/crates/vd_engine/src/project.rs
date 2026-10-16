//! Project settings (`project.vibedot`), the counterpart of Godot's
//! `project.godot` / `ProjectSettings`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vd_core::Vec2;

pub const PROJECT_FILE: &str = "project.vibedot";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub fullscreen: bool,
    pub vsync: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { width: 1280, height: 720, resizable: true, fullscreen: false, vsync: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PhysicsSettings {
    pub gravity: Vec2,
    /// Fixed physics ticks per second (Godot `physics/common/physics_ticks_per_second`).
    pub ticks_per_second: u32,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self { gravity: Vec2::new(0.0, 980.0), ticks_per_second: 60 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RenderSettings {
    /// Nearest-neighbour texture filtering (pixel art).
    pub pixel_art: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectSettings {
    pub name: String,
    /// Scene loaded at startup, relative to the project root.
    pub main_scene: String,
    pub window: WindowSettings,
    pub physics: PhysicsSettings,
    pub render: RenderSettings,
    /// Input map: action name → bindings (`"A"`, `"Space"`, `"ArrowLeft"`,
    /// `"MouseLeft"` ...).
    pub input: BTreeMap<String, Vec<String>>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            name: "New Game".into(),
            main_scene: "scenes/main.vscn".into(),
            window: WindowSettings::default(),
            physics: PhysicsSettings::default(),
            render: RenderSettings::default(),
            input: default_input_map(),
        }
    }
}

/// Godot's built-in `ui_*` actions plus a few common game actions.
pub fn default_input_map() -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    let mut add = |name: &str, keys: &[&str]| {
        m.insert(name.to_owned(), keys.iter().map(|k| (*k).to_owned()).collect());
    };
    add("ui_accept", &["Enter", "Space"]);
    add("ui_select", &["Space"]);
    add("ui_cancel", &["Escape"]);
    add("ui_left", &["ArrowLeft", "A"]);
    add("ui_right", &["ArrowRight", "D"]);
    add("ui_up", &["ArrowUp", "W"]);
    add("ui_down", &["ArrowDown", "S"]);
    add("jump", &["Space", "W", "ArrowUp"]);
    add("fire", &["J", "Z", "MouseLeft"]);
    m
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("project parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

impl ProjectSettings {
    pub fn from_json(s: &str) -> Result<Self, ProjectError> {
        Ok(serde_json::from_str(s)?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialize")
    }

    pub fn load_dir(dir: &Path) -> Result<Self, ProjectError> {
        Self::from_json(&std::fs::read_to_string(dir.join(PROJECT_FILE))?)
    }

    pub fn save_dir(&self, dir: &Path) -> Result<(), ProjectError> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(PROJECT_FILE), self.to_json())?;
        Ok(())
    }
}
