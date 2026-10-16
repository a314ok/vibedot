//! `SceneTree`: owns every node (inside the tree or orphaned) in a
//! generational arena and implements Godot's `Node`/`SceneTree` semantics:
//!
//! * nodes can exist outside the tree (`create` ≈ `memnew`) and are attached
//!   with `add_child`, detached with `remove_child`, destroyed with `free`;
//! * child names are made unique the way Godot does it (`@Node@2` for
//!   automatic names, `Enemy2` with `force_readable_name`);
//! * notifications follow Godot's order: `ENTER_TREE` pre-order, `READY`
//!   post-order (once), `EXIT_TREE` children-first in reverse order;
//! * `process_mode` / `paused`, process priorities, groups, `NodePath`
//!   resolution (`..`, `/root/..`, `%Unique`), `find_child` wildcards;
//! * signals: `connect` / `emit_signal` with `ONE_SHOT` and bound arguments.
//!
//! Because Rust code cannot call back into "scripts" while the tree is
//! borrowed, notifications and signal calls are queued as [`TreeEvent`]s in
//! order; the engine drains them right away (see `vd_engine`).

use std::collections::HashMap;

use vd_core::gd::string as gstr;
use vd_core::{Affine2, Arena, Color, ObjectId, StringName, Variant, Vec2};

use crate::kinds::PlainNode;
use crate::node::{Node, NodeId, NodeKind, ProcessMode};
use crate::packed::NodeData;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("node does not exist (stale handle?)")]
    InvalidNode,
    #[error("can't add child '{0}' to itself or to one of its descendants")]
    Cycle(String),
    #[error("can't add child '{0}': it already has a parent")]
    HasParent(String),
    #[error("the root node cannot be removed, freed or reparented")]
    Root,
    #[error("node is not a child of this parent")]
    NotAChild,
    #[error("signal '{0}' is already connected to the given method")]
    AlreadyConnected(String),
    #[error("signal connection not found")]
    NotConnected,
}

/// Godot notifications delivered through [`TreeEvent::Notification`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notification {
    EnterTree,
    Ready,
    ExitTree,
    Paused,
    Unpaused,
}

/// `Object.ConnectFlags`.
pub mod connect_flags {
    pub const DEFERRED: u32 = 1;
    pub const PERSIST: u32 = 2;
    pub const ONE_SHOT: u32 = 4;
    pub const REFERENCE_COUNTED: u32 = 8;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub signal: StringName,
    pub target: NodeId,
    pub method: StringName,
    pub flags: u32,
    /// `Callable.bind()` arguments appended after the signal arguments.
    pub binds: Vec<Variant>,
    refcount: u32,
}

/// Something the engine must dispatch, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeEvent {
    Notification(NodeId, Notification),
    /// A connected signal fired: call `method` on `target` with `args`.
    Call {
        source: NodeId,
        signal: StringName,
        target: NodeId,
        method: StringName,
        args: Vec<Variant>,
    },
}

#[derive(Clone, Debug)]
pub struct SceneTree {
    nodes: Arena<Node>,
    root: NodeId,
    current_scene: Option<NodeId>,
    queued: Vec<NodeId>,
    paused: bool,
    /// Counter for automatic `@Class@N` names (`node_hrcr_count`).
    hrcr_count: u32,
    events: Vec<TreeEvent>,
    connections: HashMap<NodeId, Vec<Connection>>,
}

impl Default for SceneTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Built-in signals every node has (for editor UIs).
pub const NODE_SIGNALS: &[&str] =
    &["ready", "tree_entered", "tree_exiting", "tree_exited", "renamed", "child_entered_tree", "child_exiting_tree", "child_order_changed"];

impl SceneTree {
    /// Empty tree with a `root` node (Godot's root `Window`).
    pub fn new() -> Self {
        let mut nodes = Arena::with_capacity(64);
        let mut root = Node::new("root", NodeKind::Node(PlainNode::default()));
        root.inside_tree = true;
        root.ready = true;
        root.ready_first = false;
        let root = nodes.insert(root);
        Self {
            nodes,
            root,
            current_scene: None,
            queued: Vec::new(),
            paused: false,
            hrcr_count: 1,
            events: Vec::new(),
            connections: HashMap::new(),
        }
    }

    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn current_scene(&self) -> Option<NodeId> {
        self.current_scene.filter(|&id| self.contains(id))
    }

    pub fn set_current_scene(&mut self, id: Option<NodeId>) {
        self.current_scene = id;
    }

    /// Current scene, or the root if there is none.
    pub fn scene_root(&self) -> NodeId {
        self.current_scene().unwrap_or(self.root)
    }

    /// Number of nodes inside the tree (`get_node_count()`).
    pub fn get_node_count(&self) -> usize {
        self.nodes.iter().filter(|(_, n)| n.inside_tree).count()
    }

    /// Total nodes alive, including orphans.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[inline]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    #[inline]
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains(id)
    }

    pub fn get2_mut(&mut self, a: NodeId, b: NodeId) -> Option<(&mut Node, &mut Node)> {
        self.nodes.get2_mut(a, b)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> + '_ {
        self.nodes.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (NodeId, &mut Node)> + '_ {
        self.nodes.iter_mut()
    }

    pub fn is_inside_tree(&self, id: NodeId) -> bool {
        self.nodes.get(id).is_some_and(|n| n.inside_tree)
    }

    // ---------------------------------------------------------------- object ids

    pub fn object_id(id: NodeId) -> ObjectId {
        ObjectId(id.to_bits())
    }

    pub fn from_object(&self, oid: ObjectId) -> Option<NodeId> {
        let id = NodeId::from_bits(oid.0)?;
        self.contains(id).then_some(id)
    }

    // ---------------------------------------------------------------- events

    /// Drains queued notifications and signal calls (in emission order).
    pub fn take_events(&mut self) -> Vec<TreeEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    fn notify(&mut self, id: NodeId, what: Notification) {
        self.events.push(TreeEvent::Notification(id, what));
    }

    // ---------------------------------------------------------------- hierarchy queries

    pub fn get_parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id)?.parent
    }

    /// Alias of [`SceneTree::get_parent`].
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.get_parent(id)
    }

    pub fn get_children(&self, id: NodeId) -> &[NodeId] {
        self.nodes.get(id).map_or(&[], |n| &n.children)
    }

    /// Alias of [`SceneTree::get_children`].
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.get_children(id)
    }

    pub fn get_child_count(&self, id: NodeId) -> usize {
        self.get_children(id).len()
    }

    /// `get_child(index)`; negative indices count from the end.
    pub fn get_child(&self, id: NodeId, index: i32) -> Option<NodeId> {
        let children = self.get_children(id);
        let len = children.len() as i32;
        let i = if index < 0 { index + len } else { index };
        if i < 0 || i >= len {
            log::error!("Index p_index = {index} is out of bounds (child count = {len}).");
            return None;
        }
        Some(children[i as usize])
    }

    /// Position among siblings, `-1` without a parent.
    pub fn get_index(&self, id: NodeId) -> i32 {
        self.get_parent(id).and_then(|p| self.get_children(p).iter().position(|&c| c == id)).map_or(-1, |i| i as i32)
    }

    pub fn is_ancestor_of(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut cur = self.get_parent(node);
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = self.get_parent(p);
        }
        false
    }

    /// Depth-first pre-order list starting at `from` (inclusive). Iterative,
    /// so arbitrarily deep trees cannot overflow the call stack.
    pub fn preorder(&self, from: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![from];
        while let Some(id) = stack.pop() {
            if let Some(n) = self.nodes.get(id) {
                out.push(id);
                stack.extend(n.children.iter().rev());
            }
        }
        out
    }

    /// Every node inside the tree, in tree order.
    pub fn all_nodes(&self) -> Vec<NodeId> {
        self.preorder(self.root)
    }

    // ---------------------------------------------------------------- naming

    fn generate_serial_child_name(&self, parent: NodeId, child: NodeId, name: &str) -> String {
        let class = self.nodes[child].get_class();
        let name = if name.is_empty() { class } else { name };
        let taken = |candidate: &str| self.get_children(parent).iter().any(|&c| c != child && self.nodes[c].name == candidate);
        if !taken(name) {
            return name.to_owned();
        }
        // Split trailing digits: "Enemy12" -> ("Enemy", "12"). Godot's default
        // name/number separator is empty, so any trailing digits count.
        let digits = name.chars().rev().take_while(char::is_ascii_digit).count();
        let split = name.len() - digits;
        let base = &name[..split];
        let mut nums = name[split..].to_owned();
        loop {
            let attempt = format!("{base}{nums}");
            if !taken(&attempt) {
                return attempt;
            }
            nums = if nums.is_empty() { "2".to_owned() } else { gstr::increase_numeric_string(&nums) };
        }
    }

    /// `_validate_child_name`: makes `child`'s name unique under `parent`.
    fn validate_child_name(&mut self, parent: NodeId, child: NodeId, force_readable: bool) {
        let current = self.nodes[child].name.clone();
        let new_name = if force_readable {
            self.generate_serial_child_name(parent, child, &current)
        } else {
            let unique = !current.is_empty() && !self.get_children(parent).iter().any(|&c| c != child && self.nodes[c].name == current);
            if unique {
                current
            } else {
                let n = self.hrcr_count;
                self.hrcr_count += 1;
                format!("@{}@{n}", self.nodes[child].get_class())
            }
        };
        self.nodes[child].name = new_name;
    }

    /// `set_name()`: validates characters and keeps sibling names unique.
    pub fn set_name(&mut self, id: NodeId, name: &str) -> Option<String> {
        if name.is_empty() {
            log::error!("Condition \"p_name.is_empty()\" is true.");
            return None;
        }
        let old = self.nodes.get(id)?.name.clone();
        self.nodes[id].name = gstr::validate_node_name(name);
        if let Some(p) = self.get_parent(id) {
            self.validate_child_name(p, id, true);
        }
        let new = self.nodes[id].name.clone();
        if new != old && self.nodes[id].inside_tree {
            self.emit_signal(id, "renamed", &[]);
        }
        Some(new)
    }

    /// Alias kept for the editor.
    pub fn rename(&mut self, id: NodeId, name: &str) -> Option<String> {
        self.set_name(id, name)
    }

    // ---------------------------------------------------------------- creation / attachment

    /// `memnew(Node)`: creates an orphan node (not inside the tree).
    pub fn create(&mut self, mut node: Node) -> NodeId {
        node.parent = None;
        node.children.clear();
        node.inside_tree = false;
        node.ready = false;
        node.queued_free = false;
        node.name = gstr::validate_node_name(&node.name);
        self.nodes.insert(node)
    }

    /// `parent.add_child(child, force_readable_name)`.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId, force_readable_name: bool) -> Result<(), TreeError> {
        if !self.contains(parent) || !self.contains(child) {
            return Err(TreeError::InvalidNode);
        }
        let child_name = self.nodes[child].name.clone();
        if child == parent || self.is_ancestor_of(child, parent) {
            return Err(TreeError::Cycle(child_name));
        }
        if self.nodes[child].parent.is_some() || child == self.root {
            return Err(TreeError::HasParent(child_name));
        }
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
        self.validate_child_name(parent, child, force_readable_name);
        // Fresh global transform so the node is usable immediately.
        let (pg, pm, pv) = {
            let p = &self.nodes[parent];
            (p.global, p.global_modulate, p.global_visible)
        };
        let c = &mut self.nodes[child];
        c.global = if c.kind.is_2d() { pg * c.transform.to_affine() } else { pg };
        c.global_modulate = pm * c.modulate;
        c.global_visible = pv && c.visible;
        if self.nodes[parent].inside_tree {
            self.propagate_enter_tree(child);
            self.propagate_ready(child);
        }
        self.emit_signal(parent, "child_order_changed", &[]);
        Ok(())
    }

    /// Convenience: `create` + `add_child(.., force_readable_name = true)`.
    pub fn add_new_child(&mut self, parent: NodeId, node: Node) -> Result<NodeId, TreeError> {
        if !self.contains(parent) {
            return Err(TreeError::InvalidNode);
        }
        let id = self.create(node);
        self.add_child(parent, id, true)?;
        Ok(id)
    }

    /// `node.add_sibling(sibling)`: inserted right after `node`.
    pub fn add_sibling(&mut self, node: NodeId, sibling: NodeId, force_readable_name: bool) -> Result<(), TreeError> {
        let parent = self.get_parent(node).ok_or(TreeError::NotAChild)?;
        if sibling == node {
            return Err(TreeError::Cycle(self.nodes[node].name.clone()));
        }
        self.add_child(parent, sibling, force_readable_name)?;
        let idx = self.get_index(node) + 1;
        self.move_child(parent, sibling, idx);
        Ok(())
    }

    fn propagate_enter_tree(&mut self, id: NodeId) {
        let parent = self.nodes[id].parent;
        self.nodes[id].inside_tree = true;
        self.notify(id, Notification::EnterTree);
        self.emit_signal(id, "tree_entered", &[]);
        if let Some(p) = parent {
            let arg = Variant::Object(Self::object_id(id));
            self.emit_signal(p, "child_entered_tree", &[arg]);
        }
        for c in self.nodes[id].children.clone() {
            if self.contains(c) && !self.nodes[c].inside_tree {
                self.propagate_enter_tree(c);
            }
        }
    }

    fn propagate_ready(&mut self, id: NodeId) {
        self.nodes[id].ready = true;
        for c in self.nodes[id].children.clone() {
            if self.contains(c) {
                self.propagate_ready(c);
            }
        }
        if self.nodes[id].ready_first {
            self.nodes[id].ready_first = false;
            self.notify(id, Notification::Ready);
            self.emit_signal(id, "ready", &[]);
        }
    }

    fn propagate_exit_tree(&mut self, id: NodeId) {
        for c in self.nodes[id].children.clone().into_iter().rev() {
            if self.contains(c) {
                self.propagate_exit_tree(c);
            }
        }
        self.emit_signal(id, "tree_exiting", &[]);
        self.notify(id, Notification::ExitTree);
        if let Some(p) = self.nodes[id].parent {
            let arg = Variant::Object(Self::object_id(id));
            self.emit_signal(p, "child_exiting_tree", &[arg]);
        }
        let n = &mut self.nodes[id];
        n.inside_tree = false;
        n.ready = false;
    }

    fn propagate_after_exit_tree(&mut self, id: NodeId) {
        for c in self.nodes[id].children.clone().into_iter().rev() {
            if self.contains(c) {
                self.propagate_after_exit_tree(c);
            }
        }
        self.emit_signal(id, "tree_exited", &[]);
    }

    /// `parent.remove_child(child)`: the subtree becomes an orphan (still alive).
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        if !self.contains(parent) || !self.contains(child) {
            return Err(TreeError::InvalidNode);
        }
        if self.nodes[child].parent != Some(parent) {
            return Err(TreeError::NotAChild);
        }
        let was_inside = self.nodes[child].inside_tree;
        if was_inside {
            self.propagate_exit_tree(child);
        }
        self.nodes[parent].children.retain(|&c| c != child);
        self.nodes[child].parent = None;
        if was_inside {
            self.propagate_after_exit_tree(child);
        }
        if self.current_scene == Some(child) {
            self.current_scene = None;
        }
        self.emit_signal(parent, "child_order_changed", &[]);
        Ok(())
    }

    /// `memdelete`/`free()`: detaches (if needed) and destroys the subtree.
    /// Returns every destroyed id.
    pub fn free(&mut self, id: NodeId) -> Result<Vec<NodeId>, TreeError> {
        if id == self.root {
            return Err(TreeError::Root);
        }
        if !self.contains(id) {
            return Err(TreeError::InvalidNode);
        }
        if let Some(p) = self.get_parent(id) {
            self.remove_child(p, id)?;
        }
        let doomed = self.preorder(id);
        for &d in &doomed {
            self.nodes.remove(d);
            self.connections.remove(&d);
        }
        // Drop connections targeting freed nodes.
        for list in self.connections.values_mut() {
            list.retain(|c| !doomed.contains(&c.target));
        }
        Ok(doomed)
    }

    /// Legacy alias of [`SceneTree::free`].
    pub fn remove(&mut self, id: NodeId) -> Result<Vec<NodeId>, TreeError> {
        self.free(id)
    }

    /// `queue_free()`: destroyed at [`SceneTree::flush_queued`].
    pub fn queue_free(&mut self, id: NodeId) {
        if id == self.root {
            return;
        }
        if let Some(n) = self.nodes.get_mut(id)
            && !n.queued_free
        {
            n.queued_free = true;
            self.queued.push(id);
        }
    }

    /// Frees nodes queued with `queue_free` (end of frame). Returns freed ids.
    pub fn flush_queued(&mut self) -> Vec<NodeId> {
        let mut freed = Vec::new();
        for id in std::mem::take(&mut self.queued) {
            if let Ok(mut ids) = self.free(id) {
                freed.append(&mut ids);
            }
        }
        freed
    }

    /// `parent.move_child(child, index)`; negative index counts from the end.
    pub fn move_child(&mut self, parent: NodeId, child: NodeId, index: i32) {
        if self.get_parent(child) != Some(parent) {
            log::error!("Child is not a child of this node.");
            return;
        }
        let siblings = &mut self.nodes[parent].children;
        let len = siblings.len() as i32;
        let mut to = if index < 0 { index + len } else { index };
        if to < 0 || to > len {
            log::error!("Invalid new child index: {index}.");
            return;
        }
        if to == len {
            to -= 1; // one past the end means "last"
        }
        let from = siblings.iter().position(|&c| c == child).expect("child is in parent list");
        if from as i32 == to {
            return;
        }
        siblings.remove(from);
        siblings.insert(to as usize, child);
        self.emit_signal(parent, "child_order_changed", &[]);
    }

    /// `reparent(new_parent, keep_global_transform)`; appended as last child.
    pub fn reparent(&mut self, id: NodeId, new_parent: NodeId, keep_global_transform: bool) -> Result<(), TreeError> {
        self.reparent_at(id, new_parent, None, keep_global_transform)
    }

    /// Reparent and insert at `index` (editor drag & drop).
    pub fn reparent_at(
        &mut self,
        id: NodeId,
        new_parent: NodeId,
        index: Option<usize>,
        keep_global_transform: bool,
    ) -> Result<(), TreeError> {
        if id == self.root {
            return Err(TreeError::Root);
        }
        if !self.contains(id) || !self.contains(new_parent) {
            return Err(TreeError::InvalidNode);
        }
        if id == new_parent || self.is_ancestor_of(id, new_parent) {
            return Err(TreeError::Cycle(self.nodes[id].name.clone()));
        }
        let global = self.compute_global(id);
        if let Some(p) = self.get_parent(id) {
            self.remove_child(p, id)?;
        }
        self.add_child(new_parent, id, true)?;
        if let Some(i) = index {
            self.move_child(new_parent, id, i as i32);
        }
        if keep_global_transform {
            let parent_global = self.compute_global(new_parent);
            let local = parent_global.inverse() * global;
            self.nodes[id].transform = vd_core::Transform2D::from_affine(&local);
        }
        Ok(())
    }

    /// `node.replace_by(new_node, keep_groups)`: `new_node` (an orphan) takes
    /// `node`'s place and children; `node` becomes an orphan.
    pub fn replace_by(&mut self, node: NodeId, new_node: NodeId, keep_groups: bool) -> Result<(), TreeError> {
        if self.get_parent(new_node).is_some() {
            return Err(TreeError::HasParent(self.nodes[new_node].name.clone()));
        }
        if keep_groups {
            let groups = self.nodes[node].groups.clone();
            for g in groups {
                if !self.nodes[new_node].groups.contains(&g) {
                    self.nodes[new_node].groups.push(g);
                }
            }
        }
        // Retarget connections pointing at the old node.
        for list in self.connections.values_mut() {
            for c in list.iter_mut().filter(|c| c.target == node) {
                c.target = new_node;
            }
        }
        if let Some(parent) = self.get_parent(node) {
            let index = self.get_index(node);
            self.remove_child(parent, node)?;
            self.add_child(parent, new_node, false)?;
            self.move_child(parent, new_node, index);
        }
        self.emit_signal(node, "replacing_by", &[Variant::Object(Self::object_id(new_node))]);
        for c in self.nodes[node].children.clone() {
            self.remove_child(node, c)?;
            self.add_child(new_node, c, false)?;
        }
        Ok(())
    }

    /// `duplicate()`: an orphan deep copy (names, groups, properties kept).
    pub fn duplicate(&mut self, id: NodeId) -> Option<NodeId> {
        let data = self.pack(id)?;
        Some(self.instantiate_orphan(&data))
    }

    /// Duplicates `id` and inserts the copy right after it (editor action).
    pub fn duplicate_in_place(&mut self, id: NodeId) -> Option<NodeId> {
        let copy = self.duplicate(id)?;
        self.add_sibling(id, copy, true).ok()?;
        Some(copy)
    }

    // ---------------------------------------------------------------- serialization

    fn instantiate_orphan(&mut self, data: &NodeData) -> NodeId {
        let id = self.create(data.to_node());
        for child in &data.children {
            let c = self.instantiate_orphan(child);
            self.add_child(id, c, true).expect("fresh orphan nodes");
        }
        id
    }

    /// Instantiates serialized node data under `parent` (`PackedScene.instantiate`
    /// + `add_child`). Signal connections stored in the data are restored.
    pub fn instantiate(&mut self, parent: NodeId, data: &NodeData) -> Result<NodeId, TreeError> {
        if !self.contains(parent) {
            return Err(TreeError::InvalidNode);
        }
        let id = self.instantiate_orphan(data);
        self.restore_connections(id, data);
        self.add_child(parent, id, true)?;
        Ok(id)
    }

    fn restore_connections(&mut self, scene_root: NodeId, data: &NodeData) {
        for c in &data.connections {
            let from = self.get_node_or_null(scene_root, &c.from);
            let to = self.get_node_or_null(scene_root, &c.to);
            match (from, to) {
                (Some(f), Some(t)) => {
                    let _ = self.connect_with(f, &c.signal, t, &c.method, c.flags | connect_flags::PERSIST, c.binds.clone());
                }
                _ => log::warn!("cannot restore connection {}.{} -> {}::{}", c.from, c.signal, c.to, c.method),
            }
        }
    }

    /// Serializes `id` and its subtree, including persistent connections
    /// between nodes of that subtree.
    pub fn pack(&self, id: NodeId) -> Option<NodeData> {
        let mut data = self.pack_rec(id)?;
        let members = self.preorder(id);
        for &src in &members {
            for c in self.connections.get(&src).into_iter().flatten() {
                if c.flags & connect_flags::PERSIST != 0 && members.contains(&c.target) {
                    data.connections.push(crate::packed::ConnectionData {
                        from: self.get_path_to(id, src),
                        signal: c.signal.to_string(),
                        to: self.get_path_to(id, c.target),
                        method: c.method.to_string(),
                        flags: c.flags & !connect_flags::PERSIST,
                        binds: c.binds.clone(),
                    });
                }
            }
        }
        Some(data)
    }

    fn pack_rec(&self, id: NodeId) -> Option<NodeData> {
        let n = self.nodes.get(id)?;
        let mut data = NodeData::from_node(n);
        data.children = n.children.iter().filter_map(|&c| self.pack_rec(c)).collect();
        Some(data)
    }

    /// Builds a tree whose current scene is `data` (child of `root`).
    pub fn from_data(data: &NodeData) -> Self {
        let mut tree = SceneTree::new();
        let root = tree.root;
        let scene = tree.instantiate(root, data).expect("root exists");
        tree.current_scene = Some(scene);
        tree.update_transforms();
        tree
    }

    // ---------------------------------------------------------------- paths

    /// Absolute path (`/root/Main/Player`); empty if not inside the tree.
    pub fn get_path(&self, id: NodeId) -> String {
        if !self.is_inside_tree(id) {
            log::error!("Cannot get path of node as it is not in a scene tree.");
            return String::new();
        }
        self.path_of(id)
    }

    /// Path from the topmost ancestor, regardless of being inside the tree.
    pub fn path_of(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { break };
            parts.push(n.name.as_str());
            cur = n.parent;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    /// Relative path from `from` to `to` (`get_path_to`).
    pub fn get_path_to(&self, from: NodeId, to: NodeId) -> String {
        if from == to {
            return ".".into();
        }
        let mut ancestors = Vec::new();
        let mut cur = Some(from);
        while let Some(c) = cur {
            ancestors.push(c);
            cur = self.get_parent(c);
        }
        let mut down = Vec::new();
        let mut common = Some(to);
        while let Some(c) = common {
            if ancestors.contains(&c) {
                break;
            }
            down.push(self.nodes[c].name.as_str());
            common = self.get_parent(c);
        }
        let Some(common) = common else {
            log::error!("No path can be resolved between the nodes as they share no common ancestor.");
            return String::new();
        };
        let ups = ancestors.iter().position(|&a| a == common).unwrap_or(0);
        let mut parts: Vec<&str> = std::iter::repeat_n("..", ups).collect();
        parts.extend(down.iter().rev());
        parts.join("/")
    }

    /// The scene root that "owns" `id` (child of `root` it descends from).
    fn owner_scene_of(&self, id: NodeId) -> NodeId {
        let mut cur = id;
        while let Some(p) = self.get_parent(cur) {
            if p == self.root {
                return cur;
            }
            cur = p;
        }
        cur
    }

    /// `get_node_or_null(path)`: `"Child/Grandchild"`, `"../Sibling"`, `"."`,
    /// absolute `"/root/Main/Player"`, and unique names `"%Score"`.
    pub fn get_node_or_null(&self, from: NodeId, path: &str) -> Option<NodeId> {
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        self.nodes.get(from)?;
        let (mut cur, rest) = match path.strip_prefix('/') {
            Some(abs) => {
                if !self.is_inside_tree(from) {
                    return None;
                }
                let mut parts = abs.splitn(2, '/');
                let first = parts.next().unwrap_or("");
                if self.nodes[self.root].name != first {
                    return None;
                }
                (self.root, parts.next().unwrap_or(""))
            }
            None => (from, path),
        };
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            cur = match part {
                "." => cur,
                ".." => self.get_parent(cur)?,
                unique if unique.starts_with('%') => {
                    let name = &unique[1..];
                    let owner = self.owner_scene_of(cur);
                    self.preorder(owner).into_iter().find(|&id| self.nodes[id].unique_name_in_owner && self.nodes[id].name == name)?
                }
                name => self.get_children(cur).iter().copied().find(|&c| self.nodes[c].name == name)?,
            };
        }
        Some(cur)
    }

    /// `get_node(path)`: like [`SceneTree::get_node_or_null`] but logs an error.
    pub fn get_node(&self, from: NodeId, path: &str) -> Option<NodeId> {
        let r = self.get_node_or_null(from, path);
        if r.is_none() {
            log::error!("Node not found: \"{path}\" (relative to \"{}\").", self.path_of(from));
        }
        r
    }

    /// `find_child(pattern, recursive)`: wildcard match on names, children
    /// before grandchildren of the same branch (depth-first).
    pub fn find_child(&self, id: NodeId, pattern: &str, recursive: bool) -> Option<NodeId> {
        if pattern.is_empty() {
            return None;
        }
        for &c in self.get_children(id) {
            if gstr::matches(&self.nodes[c].name, pattern) {
                return Some(c);
            }
            if recursive && let Some(r) = self.find_child(c, pattern, true) {
                return Some(r);
            }
        }
        None
    }

    /// `find_children(pattern, type, recursive)`.
    pub fn find_children(&self, id: NodeId, pattern: &str, type_name: &str, recursive: bool) -> Vec<NodeId> {
        let mut out = Vec::new();
        if pattern.is_empty() && type_name.is_empty() {
            return out;
        }
        for &c in self.get_children(id) {
            let n = &self.nodes[c];
            let name_ok = pattern.is_empty() || gstr::matches(&n.name, pattern);
            let type_ok = type_name.is_empty() || n.get_class() == type_name;
            if name_ok && type_ok {
                out.push(c);
            }
            if recursive {
                out.extend(self.find_children(c, pattern, type_name, true));
            }
        }
        out
    }

    // ---------------------------------------------------------------- groups

    /// Nodes inside the tree that belong to `group`, in tree order.
    pub fn get_nodes_in_group(&self, group: &str) -> Vec<NodeId> {
        self.nodes_in_group(StringName::new(group))
    }

    pub fn nodes_in_group(&self, group: StringName) -> Vec<NodeId> {
        self.all_nodes().into_iter().filter(|&id| self.nodes[id].in_group(group)).collect()
    }

    pub fn get_first_node_in_group(&self, group: &str) -> Option<NodeId> {
        self.first_in_group(StringName::new(group))
    }

    pub fn first_in_group(&self, group: StringName) -> Option<NodeId> {
        self.all_nodes().into_iter().find(|&id| self.nodes[id].in_group(group) && !self.nodes[id].queued_free)
    }

    pub fn has_group(&self, group: &str) -> bool {
        self.first_in_group(StringName::new(group)).is_some()
    }

    // ---------------------------------------------------------------- processing

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// `SceneTree.paused`; notifies pausable nodes.
    pub fn set_pause(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        let what = if paused { Notification::Paused } else { Notification::Unpaused };
        for id in self.all_nodes() {
            self.notify(id, what);
        }
    }

    /// Effective process mode after resolving `Inherit` up the hierarchy.
    pub fn effective_process_mode(&self, id: NodeId) -> ProcessMode {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { break };
            if n.process_mode != ProcessMode::Inherit {
                return n.process_mode;
            }
            cur = n.parent;
        }
        ProcessMode::Pausable
    }

    /// `can_process()`: inside the tree and allowed by pause state.
    pub fn can_process(&self, id: NodeId) -> bool {
        if !self.is_inside_tree(id) {
            return false;
        }
        match self.effective_process_mode(id) {
            ProcessMode::Inherit | ProcessMode::Pausable => !self.paused,
            ProcessMode::WhenPaused => self.paused,
            ProcessMode::Always => true,
            ProcessMode::Disabled => false,
        }
    }

    fn process_list(&self, enabled: impl Fn(&Node) -> bool, priority: impl Fn(&Node) -> i32) -> Vec<NodeId> {
        let mut list: Vec<(i32, usize, NodeId)> = self
            .all_nodes()
            .into_iter()
            .enumerate()
            .filter(|&(_, id)| enabled(&self.nodes[id]) && self.can_process(id))
            .map(|(order, id)| (priority(&self.nodes[id]), order, id))
            .collect();
        list.sort_unstable_by_key(|&(p, order, _)| (p, order));
        list.into_iter().map(|(_, _, id)| id).collect()
    }

    /// Nodes whose `_process` runs this frame, in priority then tree order.
    pub fn process(&self) -> Vec<NodeId> {
        self.process_list(|n| n.processing.process, |n| n.process_priority)
    }

    pub fn physics_process(&self) -> Vec<NodeId> {
        self.process_list(|n| n.processing.physics_process, |n| n.physics_process_priority)
    }

    pub fn internal_process(&self) -> Vec<NodeId> {
        self.process_list(|n| n.processing.process_internal, |n| n.process_priority)
    }

    pub fn internal_physics_process(&self) -> Vec<NodeId> {
        self.process_list(|n| n.processing.physics_process_internal, |n| n.physics_process_priority)
    }

    // ---------------------------------------------------------------- signals

    pub fn connect(&mut self, source: NodeId, signal: &str, target: NodeId, method: &str, flags: u32) -> Result<(), TreeError> {
        self.connect_with(source, signal, target, method, flags, Vec::new())
    }

    /// `connect(signal, callable.bind(binds...), flags)`.
    pub fn connect_with(
        &mut self,
        source: NodeId,
        signal: &str,
        target: NodeId,
        method: &str,
        flags: u32,
        binds: Vec<Variant>,
    ) -> Result<(), TreeError> {
        if !self.contains(source) || !self.contains(target) {
            return Err(TreeError::InvalidNode);
        }
        let signal = StringName::new(signal);
        let method = StringName::new(method);
        let list = self.connections.entry(source).or_default();
        if let Some(existing) = list.iter_mut().find(|c| c.signal == signal && c.target == target && c.method == method) {
            if flags & connect_flags::REFERENCE_COUNTED != 0 {
                existing.refcount += 1;
                return Ok(());
            }
            return Err(TreeError::AlreadyConnected(signal.to_string()));
        }
        list.push(Connection { signal, target, method, flags, binds, refcount: 1 });
        Ok(())
    }

    pub fn disconnect(&mut self, source: NodeId, signal: &str, target: NodeId, method: &str) -> Result<(), TreeError> {
        let (signal, method) = (StringName::new(signal), StringName::new(method));
        let list = self.connections.get_mut(&source).ok_or(TreeError::NotConnected)?;
        let pos =
            list.iter().position(|c| c.signal == signal && c.target == target && c.method == method).ok_or(TreeError::NotConnected)?;
        if list[pos].refcount > 1 {
            list[pos].refcount -= 1;
        } else {
            list.remove(pos);
        }
        Ok(())
    }

    pub fn is_connected(&self, source: NodeId, signal: &str, target: NodeId, method: &str) -> bool {
        let (signal, method) = (StringName::new(signal), StringName::new(method));
        self.connections.get(&source).is_some_and(|l| l.iter().any(|c| c.signal == signal && c.target == target && c.method == method))
    }

    pub fn get_signal_connection_list(&self, source: NodeId, signal: &str) -> Vec<Connection> {
        let signal = StringName::new(signal);
        self.connections.get(&source).map_or_else(Vec::new, |l| l.iter().filter(|c| c.signal == signal).cloned().collect())
    }

    pub fn all_connections(&self, source: NodeId) -> &[Connection] {
        self.connections.get(&source).map_or(&[], Vec::as_slice)
    }

    /// `emit_signal(name, args...)`: queues a call for every connection.
    /// Returns how many connections fired.
    pub fn emit_signal(&mut self, source: NodeId, signal: &str, args: &[Variant]) -> usize {
        let Some(list) = self.connections.get_mut(&source) else {
            return 0;
        };
        let signal = StringName::new(signal);
        let mut fired = Vec::new();
        list.retain(|c| {
            if c.signal != signal {
                return true;
            }
            fired.push(c.clone());
            c.flags & connect_flags::ONE_SHOT == 0
        });
        let n = fired.len();
        for c in fired {
            let mut call_args = args.to_vec();
            call_args.extend(c.binds.iter().cloned());
            self.events.push(TreeEvent::Call { source, signal, target: c.target, method: c.method, args: call_args });
        }
        n
    }

    // ---------------------------------------------------------------- transforms

    /// Recomputes cached global transforms, modulate and visibility for
    /// every node inside the tree (parents before children).
    pub fn update_transforms(&mut self) {
        let mut stack: Vec<(NodeId, Affine2, Color, bool)> = vec![(self.root, Affine2::IDENTITY, Color::WHITE, true)];
        while let Some((id, parent_xf, parent_mod, parent_vis)) = stack.pop() {
            let Some(n) = self.nodes.get_mut(id) else {
                continue;
            };
            n.global = if n.kind.is_2d() { parent_xf * n.transform.to_affine() } else { parent_xf };
            n.global_modulate = parent_mod * n.modulate;
            n.global_visible = parent_vis && n.visible;
            let (g, m, v) = (n.global, n.global_modulate, n.global_visible);
            for &c in n.children.iter().rev() {
                stack.push((c, g, m, v));
            }
        }
    }

    /// Global transform computed by walking up the hierarchy (always fresh).
    pub fn compute_global(&self, id: NodeId) -> Affine2 {
        let mut xf = Affine2::IDENTITY;
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { break };
            if n.kind.is_2d() {
                xf = n.transform.to_affine() * xf;
            }
            cur = n.parent;
        }
        xf
    }

    pub fn global_position(&self, id: NodeId) -> Vec2 {
        self.compute_global(id).translation
    }

    pub fn set_global_position(&mut self, id: NodeId, pos: Vec2) {
        let parent_xf = self.get_parent(id).map_or(Affine2::IDENTITY, |p| self.compute_global(p));
        let local = parent_xf.inverse().transform_point2(pos);
        if let Some(n) = self.nodes.get_mut(id) {
            n.transform.position = local;
            n.global.translation = pos;
        }
    }

    /// The first `WorldEnvironment` in the tree, if any.
    pub fn environment(&self) -> Option<&crate::kinds::WorldEnvironment> {
        self.all_nodes().into_iter().find_map(|id| match &self.nodes[id].kind {
            NodeKind::WorldEnvironment(env) => Some(env),
            _ => None,
        })
    }

    pub fn set_ready(&mut self, id: NodeId) {
        if let Some(n) = self.nodes.get_mut(id) {
            n.ready = true;
        }
    }
}
