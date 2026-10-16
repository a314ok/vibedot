//! The `Node`: name, hierarchy links, 2D transform and a typed payload.

use serde::{Deserialize, Serialize};
use vd_core::{Affine2, Color, Handle, StringName, Transform2D, VType, Variant};

use crate::kinds::*;
use crate::reflect::{PropHint, PropValue, PropertyInfo, Reflect};

pub type NodeId = Handle<Node>;

/// Typed payload of a node. An enum instead of trait objects: no virtual
/// dispatch, no per-node heap allocation for the payload, exhaustive matches.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    Node(PlainNode),
    Node2D(Node2D),
    Sprite2D(Sprite2D),
    Shape2D(Shape2D),
    Label(Label),
    Camera2D(Camera2D),
    PointLight2D(PointLight2D),
    Particles2D(Particles2D),
    Body2D(Body2D),
    Timer(Timer),
    WorldEnvironment(WorldEnvironment),
}

/// Static description of a node type for "Add Node" menus.
#[derive(Clone, Copy, Debug)]
pub struct KindInfo {
    pub type_name: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub description: &'static str,
}

impl NodeKind {
    /// Every creatable type, with a constructor (presets for body types).
    pub fn catalog() -> Vec<(KindInfo, NodeKind)> {
        let body = |t: BodyType, shape: CollisionShape| {
            NodeKind::Body2D(Body2D { body_type: t, shape, ..Default::default() })
        };
        let mut v = vec![
            NodeKind::Node(PlainNode::default()),
            NodeKind::Node2D(Node2D::default()),
            NodeKind::Sprite2D(Sprite2D::default()),
            NodeKind::Shape2D(Shape2D::default()),
            NodeKind::Label(Label::default()),
            NodeKind::Camera2D(Camera2D::default()),
            NodeKind::PointLight2D(PointLight2D::default()),
            NodeKind::Particles2D(Particles2D::default()),
            body(BodyType::Character, CollisionShape::Rect),
            body(BodyType::Rigid, CollisionShape::Circle),
            body(BodyType::Static, CollisionShape::Rect),
            body(BodyType::Area, CollisionShape::Circle),
            NodeKind::Timer(Timer::default()),
            NodeKind::WorldEnvironment(WorldEnvironment::default()),
        ];
        v.drain(..).map(|k| (k.info(), k)).collect()
    }

    pub fn info(&self) -> KindInfo {
        let (type_name, icon, category, description) = match self {
            NodeKind::Node(_) => ("Node", "○", "Core", "Base node without a transform"),
            NodeKind::Node2D(_) => ("Node2D", "✥", "2D", "Groups 2D children under a transform"),
            NodeKind::Sprite2D(_) => ("Sprite2D", "🖼", "2D", "Textured quad with sprite-sheet animation"),
            NodeKind::Shape2D(_) => ("Shape2D", "◆", "2D", "Anti-aliased vector shape (rect, circle, ring…)"),
            NodeKind::Label(_) => ("Label", "🅰", "2D", "Text, in world or screen space"),
            NodeKind::Camera2D(_) => ("Camera2D", "🎥", "2D", "Viewpoint with zoom, smoothing and shake"),
            NodeKind::PointLight2D(_) => ("PointLight2D", "💡", "Rendering", "Additive HDR point light"),
            NodeKind::Particles2D(_) => ("Particles2D", "✨", "Rendering", "CPU particle emitter"),
            NodeKind::Body2D(b) => match b.body_type {
                BodyType::Static => ("StaticBody2D", "▣", "Physics", "Immovable collider (walls, floors)"),
                BodyType::Character => ("CharacterBody2D", "🏃", "Physics", "Velocity-driven body that slides"),
                BodyType::Rigid => ("RigidBody2D", "⚽", "Physics", "Simulated body with gravity and bounce"),
                BodyType::Area => ("Area2D", "◌", "Physics", "Detects overlaps, no collision response"),
            },
            NodeKind::Timer(_) => ("Timer", "⏱", "Core", "Fires `On Timeout` periodically"),
            NodeKind::WorldEnvironment(_) => {
                ("WorldEnvironment", "🌌", "Rendering", "Bloom, tonemapping, vignette, CRT…")
            }
        };
        KindInfo { type_name, icon, category, description }
    }

    pub fn type_name(&self) -> &'static str {
        self.info().type_name
    }

    /// Does this node have a meaningful 2D transform?
    pub fn is_2d(&self) -> bool {
        !matches!(self, NodeKind::Node(_) | NodeKind::Timer(_) | NodeKind::WorldEnvironment(_))
    }

    fn reflect(&self) -> &dyn Reflect {
        match self {
            NodeKind::Node(k) => k,
            NodeKind::Node2D(k) => k,
            NodeKind::Sprite2D(k) => k,
            NodeKind::Shape2D(k) => k,
            NodeKind::Label(k) => k,
            NodeKind::Camera2D(k) => k,
            NodeKind::PointLight2D(k) => k,
            NodeKind::Particles2D(k) => k,
            NodeKind::Body2D(k) => k,
            NodeKind::Timer(k) => k,
            NodeKind::WorldEnvironment(k) => k,
        }
    }

    fn reflect_mut(&mut self) -> &mut dyn Reflect {
        match self {
            NodeKind::Node(k) => k,
            NodeKind::Node2D(k) => k,
            NodeKind::Sprite2D(k) => k,
            NodeKind::Shape2D(k) => k,
            NodeKind::Label(k) => k,
            NodeKind::Camera2D(k) => k,
            NodeKind::PointLight2D(k) => k,
            NodeKind::Particles2D(k) => k,
            NodeKind::Body2D(k) => k,
            NodeKind::Timer(k) => k,
            NodeKind::WorldEnvironment(k) => k,
        }
    }
}

/// A node in the scene tree.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub transform: Transform2D,
    pub visible: bool,
    pub z_index: i32,
    /// Tint multiplied into this node and all its children.
    pub modulate: Color,
    pub groups: Vec<StringName>,
    /// Path of the attached blueprint (`.vbp`), if any.
    pub script: Option<String>,
    /// Designer-defined variables (inspector "Custom" section).
    pub vars: Vec<(String, Variant)>,

    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    /// World transform, refreshed by `SceneTree::update_transforms`.
    pub(crate) global: Affine2,
    pub(crate) global_modulate: Color,
    pub(crate) global_visible: bool,
    pub(crate) queued_free: bool,
    pub(crate) ready: bool,
}

const COMMON_2D: &[PropertyInfo] = &[
    PropertyInfo { name: "position", ty: VType::Vec2, hint: PropHint::None, group: "Transform" },
    PropertyInfo { name: "rotation", ty: VType::Number, hint: PropHint::Angle, group: "Transform" },
    PropertyInfo { name: "scale", ty: VType::Vec2, hint: PropHint::None, group: "Transform" },
    PropertyInfo { name: "z_index", ty: VType::Number, hint: PropHint::Range { min: -100.0, max: 100.0, step: 1.0 }, group: "Visibility" },
    PropertyInfo { name: "visible", ty: VType::Bool, hint: PropHint::None, group: "Visibility" },
    PropertyInfo { name: "modulate", ty: VType::Color, hint: PropHint::HdrColor, group: "Visibility" },
];

impl Node {
    pub fn new(name: impl Into<String>, kind: NodeKind) -> Self {
        Self {
            name: name.into(),
            kind,
            transform: Transform2D::IDENTITY,
            visible: true,
            z_index: 0,
            modulate: Color::WHITE,
            groups: Vec::new(),
            script: None,
            vars: Vec::new(),
            parent: None,
            children: Vec::new(),
            global: Affine2::IDENTITY,
            global_modulate: Color::WHITE,
            global_visible: true,
            queued_free: false,
            ready: false,
        }
    }

    pub fn with_position(mut self, p: vd_core::Vec2) -> Self {
        self.transform.position = p;
        self
    }

    pub fn with_script(mut self, path: impl Into<String>) -> Self {
        self.script = Some(path.into());
        self
    }

    pub fn with_group(mut self, g: &str) -> Self {
        self.groups.push(StringName::new(g));
        self
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    pub fn global_transform(&self) -> Affine2 {
        self.global
    }

    pub fn global_position(&self) -> vd_core::Vec2 {
        self.global.translation
    }

    pub fn global_modulate(&self) -> Color {
        self.global_modulate
    }

    /// Visible itself and all ancestors visible.
    pub fn is_visible_in_tree(&self) -> bool {
        self.global_visible
    }

    pub fn is_queued_for_deletion(&self) -> bool {
        self.queued_free
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn in_group(&self, g: StringName) -> bool {
        self.groups.contains(&g)
    }

    pub fn var(&self, name: &str) -> Option<&Variant> {
        self.vars.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// All editable properties (common + kind specific).
    pub fn property_list(&self) -> Vec<PropertyInfo> {
        let mut out = Vec::with_capacity(24);
        if self.kind.is_2d() {
            out.extend_from_slice(COMMON_2D);
        }
        self.kind.reflect().properties(&mut out);
        out
    }

    pub fn get(&self, name: &str) -> Option<Variant> {
        match name {
            "name" => Some(Variant::str(&self.name)),
            "position" => Some(self.transform.position.to_variant()),
            "rotation" => Some(self.transform.rotation.to_variant()),
            "rotation_degrees" => Some(self.transform.rotation.to_degrees().to_variant()),
            "scale" => Some(self.transform.scale.to_variant()),
            "global_position" => Some(self.global.translation.to_variant()),
            "z_index" => Some(self.z_index.to_variant()),
            "visible" => Some(self.visible.to_variant()),
            "modulate" => Some(self.modulate.to_variant()),
            _ => self.kind.reflect().get_prop(name).or_else(|| self.var(name).cloned()),
        }
    }

    /// Sets a property; unknown names become custom variables (like Godot
    /// metadata), so blueprints can stash state on any node.
    pub fn set(&mut self, name: &str, value: &Variant) -> bool {
        match name {
            "name" => self.name = value.to_string(),
            "position" => self.transform.position = value.to_vec2(),
            "rotation" => self.transform.rotation = value.to_f32(),
            "rotation_degrees" => self.transform.rotation = value.to_f32().to_radians(),
            "scale" => self.transform.scale = value.to_vec2(),
            "z_index" => self.z_index = i32::from_variant(value),
            "visible" => self.visible = value.to_bool(),
            "modulate" => self.modulate = value.to_color(),
            _ => {
                if !self.kind.reflect_mut().set_prop(name, value) {
                    if let Some(slot) = self.vars.iter_mut().find(|(n, _)| n == name) {
                        slot.1 = value.clone();
                    } else {
                        self.vars.push((name.to_owned(), value.clone()));
                    }
                }
            }
        }
        true
    }
}
