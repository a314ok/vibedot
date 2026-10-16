//! Scene serialization (`.vscn`, JSON) - the counterpart of Godot's
//! `PackedScene` / `.tscn`. JSON keeps scenes diff-friendly in git.

use std::path::Path;

use serde::{Deserialize, Serialize};
use vd_core::{Color, StringName, Transform2D, Variant};

use crate::node::{Node, NodeKind};
use crate::tree::SceneTree;

pub const SCENE_FORMAT_VERSION: u32 = 1;

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
fn is_true(v: &bool) -> bool {
    *v
}
fn default_true() -> bool {
    true
}
fn is_white(c: &Color) -> bool {
    *c == Color::WHITE
}
fn is_identity(t: &Transform2D) -> bool {
    *t == Transform2D::IDENTITY
}

/// Serializable form of one node and its children.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeData {
    pub name: String,
    #[serde(flatten)]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "is_identity")]
    pub transform: Transform2D,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub z_index: i32,
    #[serde(default = "white", skip_serializing_if = "is_white")]
    pub modulate: Color,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<StringName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars: Vec<(String, Variant)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeData>,
}

fn white() -> Color {
    Color::WHITE
}

impl NodeData {
    pub fn from_node(n: &Node) -> Self {
        Self {
            name: n.name.clone(),
            kind: n.kind.clone(),
            transform: n.transform,
            visible: n.visible,
            z_index: n.z_index,
            modulate: n.modulate,
            groups: n.groups.clone(),
            script: n.script.clone(),
            vars: n.vars.clone(),
            children: Vec::new(),
        }
    }

    pub fn to_node(&self) -> Node {
        let mut n = Node::new(self.name.clone(), self.kind.clone());
        n.transform = self.transform;
        n.visible = self.visible;
        n.z_index = self.z_index;
        n.modulate = self.modulate;
        n.groups.clone_from(&self.groups);
        n.script.clone_from(&self.script);
        n.vars.clone_from(&self.vars);
        n
    }
}

/// On-disk scene file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneFile {
    pub format: u32,
    pub root: NodeData,
}

#[derive(Debug, thiserror::Error)]
pub enum SceneIoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("scene parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unsupported scene format version {0}")]
    Version(u32),
}

impl SceneFile {
    pub fn from_tree(tree: &SceneTree) -> Self {
        Self { format: SCENE_FORMAT_VERSION, root: tree.pack(tree.root()).expect("root always exists") }
    }

    pub fn to_tree(&self) -> SceneTree {
        SceneTree::from_data(&self.root)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("scene serialization cannot fail")
    }

    pub fn from_json(s: &str) -> Result<Self, SceneIoError> {
        let f: SceneFile = serde_json::from_str(s)?;
        if f.format > SCENE_FORMAT_VERSION {
            return Err(SceneIoError::Version(f.format));
        }
        Ok(f)
    }

    pub fn load(path: &Path) -> Result<Self, SceneIoError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), SceneIoError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write-then-rename: a crash mid-save never leaves a truncated scene.
        let tmp = path.with_extension("vscn.tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
