//! Native Rust scripts - Vibedot's answer to GDScript / GDExtension.
//!
//! Attach a script to a node by setting its `script` to `"rust:Name"` and
//! registering a factory under `Name`:
//!
//! ```ignore
//! struct Spinner { speed: f32 }
//! impl NodeScript for Spinner {
//!     fn process(&mut self, cx: &mut ScriptCtx<'_>, delta: f32) {
//!         if let Some(n) = cx.node_mut() { n.transform.rotation += self.speed * delta; }
//!     }
//! }
//! registry.register("Spinner", || Box::new(Spinner { speed: 2.0 }));
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use vd_core::{StringName, Variant, Vec2};
use vd_scene::{Node, NodeId, SceneTree};

use crate::game::HostState;

/// Context handed to script callbacks.
pub struct ScriptCtx<'a> {
    pub tree: &'a mut SceneTree,
    pub host: &'a mut HostState,
    /// The node this script is attached to.
    pub node: NodeId,
}

impl ScriptCtx<'_> {
    pub fn node(&self) -> Option<&Node> {
        self.tree.get(self.node)
    }

    pub fn node_mut(&mut self) -> Option<&mut Node> {
        self.tree.get_mut(self.node)
    }

    pub fn get_node(&self, path: &str) -> Option<NodeId> {
        self.tree.get_node(self.node, path)
    }

    pub fn is_action_pressed(&self, action: &str) -> bool {
        self.host.is_action_pressed(StringName::new(action))
    }

    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        self.host.is_action_just_pressed(StringName::new(action))
    }

    pub fn get_axis(&self, negative: &str, positive: &str) -> f32 {
        self.host.input_map.get_axis(&self.host.input, StringName::new(negative), StringName::new(positive))
    }

    pub fn mouse_position(&self) -> Vec2 {
        self.host.mouse_world()
    }

    pub fn emit_signal(&mut self, signal: &str, args: &[Variant]) {
        self.tree.emit_signal(self.node, signal, args);
    }

    pub fn print(&mut self, msg: impl AsRef<str>) {
        self.host.info(msg.as_ref());
    }

    /// Instantiates a scene file under the current scene.
    pub fn spawn(&mut self, scene: &str, position: Vec2) -> Option<NodeId> {
        self.host.spawn(self.tree, scene, None, position)
    }

    pub fn queue_free(&mut self) {
        self.tree.queue_free(self.node);
    }
}

/// Behaviour attached to a node. All callbacks are optional.
pub trait NodeScript: Send {
    /// `_enter_tree`.
    fn enter_tree(&mut self, _cx: &mut ScriptCtx<'_>) {}
    /// `_ready`.
    fn ready(&mut self, _cx: &mut ScriptCtx<'_>) {}
    /// `_process(delta)`.
    fn process(&mut self, _cx: &mut ScriptCtx<'_>, _delta: f32) {}
    /// `_physics_process(delta)`.
    fn physics_process(&mut self, _cx: &mut ScriptCtx<'_>, _delta: f32) {}
    /// `_exit_tree`.
    fn exit_tree(&mut self, _cx: &mut ScriptCtx<'_>) {}
    /// A connected signal or `Emit Event` called `method`. Return `true` if handled.
    fn call(&mut self, _cx: &mut ScriptCtx<'_>, _method: &str, _args: &[Variant]) -> bool {
        false
    }
    /// Whether `process` should run every frame (`set_process`).
    fn wants_process(&self) -> bool {
        true
    }
    fn wants_physics_process(&self) -> bool {
        false
    }
}

pub type ScriptFactory = Arc<dyn Fn() -> Box<dyn NodeScript> + Send + Sync>;

/// Name → factory map for native scripts.
#[derive(Clone, Default)]
pub struct ScriptRegistry {
    factories: HashMap<String, ScriptFactory>,
}

impl std::fmt::Debug for ScriptRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.factories.keys()).finish()
    }
}

impl ScriptRegistry {
    pub fn register(&mut self, name: &str, factory: impl Fn() -> Box<dyn NodeScript> + Send + Sync + 'static) {
        self.factories.insert(name.to_owned(), Arc::new(factory));
    }

    pub fn create(&self, name: &str) -> Option<Box<dyn NodeScript>> {
        self.factories.get(name).map(|f| f())
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.factories.keys().cloned().collect();
        v.sort();
        v
    }
}
