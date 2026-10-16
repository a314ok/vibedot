//! `SceneTree`: owns every node in a generational arena and maintains the
//! parent/child hierarchy. Equivalent to Godot's `SceneTree` + the tree parts
//! of `Node` (`add_child`, `get_node`, `queue_free`, groups, ...).

use vd_core::{Affine2, Arena, Color, ObjectId, StringName, Vec2};

use crate::kinds::Node2D;
use crate::node::{Node, NodeId, NodeKind};
use crate::packed::NodeData;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("node does not exist (stale handle?)")]
    InvalidNode,
    #[error("cannot move a node into its own subtree")]
    Cycle,
    #[error("the root node cannot be removed or reparented")]
    Root,
}

#[derive(Clone, Debug)]
pub struct SceneTree {
    nodes: Arena<Node>,
    root: NodeId,
    queued: Vec<NodeId>,
}

impl Default for SceneTree {
    fn default() -> Self {
        Self::new("Root")
    }
}

impl SceneTree {
    pub fn new(root_name: &str) -> Self {
        Self::with_root(Node::new(root_name, NodeKind::Node2D(Node2D::default())))
    }

    pub fn with_root(root: Node) -> Self {
        let mut nodes = Arena::with_capacity(64);
        let root = nodes.insert(root);
        Self { nodes, root, queued: Vec::new() }
    }

    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

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

    /// Two distinct nodes mutably at once.
    pub fn get2_mut(&mut self, a: NodeId, b: NodeId) -> Option<(&mut Node, &mut Node)> {
        self.nodes.get2_mut(a, b)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> + '_ {
        self.nodes.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (NodeId, &mut Node)> + '_ {
        self.nodes.iter_mut()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes.get(id).map_or(&[], |n| &n.children)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id)?.parent
    }

    // ---------------------------------------------------------------- object ids

    pub fn object_id(id: NodeId) -> ObjectId {
        ObjectId(id.to_bits())
    }

    /// Resolve an `ObjectId` back to a live node (None if freed).
    pub fn from_object(&self, oid: ObjectId) -> Option<NodeId> {
        let id = NodeId::from_bits(oid.0)?;
        self.contains(id).then_some(id)
    }

    // ---------------------------------------------------------------- mutation

    fn unique_child_name(&self, parent: NodeId, wanted: &str, ignore: Option<NodeId>) -> String {
        let taken = |name: &str| {
            self.children(parent).iter().any(|&c| Some(c) != ignore && self.nodes[c].name == name)
        };
        if !taken(wanted) {
            return wanted.to_owned();
        }
        // Godot style: "Enemy", "Enemy2", "Enemy3", ...
        let base = wanted.trim_end_matches(|c: char| c.is_ascii_digit());
        let base = if base.is_empty() { wanted } else { base };
        (2..).map(|i| format!("{base}{i}")).find(|n| !taken(n)).expect("infinite iterator")
    }

    /// Adds `node` as the last child of `parent`. The name is made unique
    /// among siblings.
    pub fn add_child(&mut self, parent: NodeId, mut node: Node) -> Result<NodeId, TreeError> {
        if !self.contains(parent) {
            return Err(TreeError::InvalidNode);
        }
        node.name = self.unique_child_name(parent, &node.name, None);
        node.parent = Some(parent);
        node.children.clear();
        node.ready = false;
        node.queued_free = false;
        // Pre-compute the global transform so the node is usable immediately.
        let p = &self.nodes[parent];
        node.global = p.global * node.transform.to_affine();
        node.global_modulate = p.global_modulate * node.modulate;
        node.global_visible = p.global_visible && node.visible;
        let id = self.nodes.insert(node);
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// Instantiates serialized node data (and its children) under `parent`.
    pub fn instantiate(&mut self, parent: NodeId, data: &NodeData) -> Result<NodeId, TreeError> {
        let id = self.add_child(parent, data.to_node())?;
        for child in &data.children {
            self.instantiate(id, child)?;
        }
        Ok(id)
    }

    /// Renames a node keeping sibling names unique. Returns the final name.
    pub fn rename(&mut self, id: NodeId, name: &str) -> Option<String> {
        let parent = self.parent(id);
        let name = match parent {
            Some(p) => self.unique_child_name(p, name, Some(id)),
            None => name.to_owned(),
        };
        self.get_mut(id)?.name.clone_from(&name);
        Some(name)
    }

    /// Immediately removes `id` and its whole subtree. Returns removed ids.
    pub fn remove(&mut self, id: NodeId) -> Result<Vec<NodeId>, TreeError> {
        if id == self.root {
            return Err(TreeError::Root);
        }
        let parent = self.nodes.get(id).ok_or(TreeError::InvalidNode)?.parent;
        if let Some(p) = parent.and_then(|p| self.nodes.get_mut(p)) {
            p.children.retain(|&c| c != id);
        }
        let doomed = self.preorder(id);
        for &d in &doomed {
            self.nodes.remove(d);
        }
        Ok(doomed)
    }

    /// Godot's `queue_free()`: marks the node, removal happens at `flush_queued`.
    pub fn queue_free(&mut self, id: NodeId) {
        if id == self.root {
            return;
        }
        if let Some(n) = self.nodes.get_mut(id) {
            if !n.queued_free {
                n.queued_free = true;
                self.queued.push(id);
            }
        }
    }

    /// Removes all nodes queued for deletion; returns every freed id.
    pub fn flush_queued(&mut self) -> Vec<NodeId> {
        let mut freed = Vec::new();
        for id in std::mem::take(&mut self.queued) {
            if let Ok(mut ids) = self.remove(id) {
                freed.append(&mut ids);
            }
        }
        freed
    }

    pub fn is_ancestor_of(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        while let Some(p) = self.parent(node) {
            if p == ancestor {
                return true;
            }
            node = p;
        }
        false
    }

    /// Moves `id` under `new_parent` at `index` (clamped). Keeps the global
    /// transform if `keep_global` is set (like dragging in Godot's dock).
    pub fn reparent(
        &mut self,
        id: NodeId,
        new_parent: NodeId,
        index: Option<usize>,
        keep_global: bool,
    ) -> Result<(), TreeError> {
        if id == self.root {
            return Err(TreeError::Root);
        }
        if !self.contains(id) || !self.contains(new_parent) {
            return Err(TreeError::InvalidNode);
        }
        if id == new_parent || self.is_ancestor_of(id, new_parent) {
            return Err(TreeError::Cycle);
        }
        let global = self.compute_global(id);
        let old_parent = self.parent(id).ok_or(TreeError::Root)?;
        self.nodes[old_parent].children.retain(|&c| c != id);
        let name = self.unique_child_name(new_parent, &self.nodes[id].name.clone(), Some(id));
        let siblings = &mut self.nodes[new_parent].children;
        let idx = index.unwrap_or(siblings.len()).min(siblings.len());
        siblings.insert(idx, id);
        let parent_global = self.compute_global(new_parent);
        let node = &mut self.nodes[id];
        node.parent = Some(new_parent);
        node.name = name;
        if keep_global {
            let local = parent_global.inverse() * global;
            node.transform = vd_core::Transform2D::from_affine(&local);
        }
        Ok(())
    }

    /// Changes the index of `id` among its siblings.
    pub fn move_child(&mut self, id: NodeId, new_index: usize) {
        if let Some(p) = self.parent(id) {
            let siblings = &mut self.nodes[p].children;
            if let Some(pos) = siblings.iter().position(|&c| c == id) {
                siblings.remove(pos);
                let i = new_index.min(siblings.len());
                siblings.insert(i, id);
            }
        }
    }

    pub fn set_ready(&mut self, id: NodeId) {
        if let Some(n) = self.nodes.get_mut(id) {
            n.ready = true;
        }
    }

    // ---------------------------------------------------------------- traversal

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

    pub fn all_nodes(&self) -> Vec<NodeId> {
        self.preorder(self.root)
    }

    pub fn nodes_in_group(&self, group: StringName) -> Vec<NodeId> {
        self.all_nodes().into_iter().filter(|&id| self.nodes[id].in_group(group)).collect()
    }

    pub fn first_in_group(&self, group: StringName) -> Option<NodeId> {
        self.all_nodes().into_iter().find(|&id| self.nodes[id].in_group(group) && !self.nodes[id].queued_free)
    }

    pub fn find_child(&self, from: NodeId, name: &str, recursive: bool) -> Option<NodeId> {
        if recursive {
            self.preorder(from).into_iter().skip(1).find(|&id| self.nodes[id].name == name)
        } else {
            self.children(from).iter().copied().find(|&c| self.nodes[c].name == name)
        }
    }

    /// Resolves a Godot-style `NodePath`: `"Child/Grandchild"`, `"../Sibling"`,
    /// `"."`, absolute `"/Root/Player"`, or a unique `"%Name"` lookup.
    pub fn get_node(&self, from: NodeId, path: &str) -> Option<NodeId> {
        let path = path.trim();
        if let Some(unique) = path.strip_prefix('%') {
            return self.find_child(self.root, unique, true).or_else(|| {
                (self.nodes.get(self.root)?.name == unique).then_some(self.root)
            });
        }
        let (mut cur, rest) = match path.strip_prefix('/') {
            Some(abs) => {
                let mut parts = abs.splitn(2, '/');
                let first = parts.next().unwrap_or("");
                if self.nodes.get(self.root)?.name != first {
                    return None;
                }
                (self.root, parts.next().unwrap_or(""))
            }
            None => (from, path),
        };
        self.nodes.get(cur)?;
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            cur = match part {
                "." => cur,
                ".." => self.parent(cur)?,
                name => self.find_child(cur, name, false)?,
            };
        }
        Some(cur)
    }

    /// Absolute path like `/Root/Player/Sprite`.
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

    // ---------------------------------------------------------------- transforms

    /// Recomputes cached global transforms, modulate and visibility for the
    /// whole tree in one pre-order sweep (parents before children).
    pub fn update_transforms(&mut self) {
        let mut stack: Vec<(NodeId, Affine2, Color, bool)> =
            vec![(self.root, Affine2::IDENTITY, Color::WHITE, true)];
        while let Some((id, parent_xf, parent_mod, parent_vis)) = stack.pop() {
            let Some(n) = self.nodes.get_mut(id) else { continue };
            n.global = if n.kind.is_2d() { parent_xf * n.transform.to_affine() } else { parent_xf };
            n.global_modulate = parent_mod * n.modulate;
            n.global_visible = parent_vis && n.visible;
            let (g, m, v) = (n.global, n.global_modulate, n.global_visible);
            for &c in n.children.iter().rev() {
                stack.push((c, g, m, v));
            }
        }
    }

    /// Global transform computed by walking up the hierarchy (always fresh,
    /// unlike the cached value which is updated once per frame).
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
        let parent_xf = self.parent(id).map_or(Affine2::IDENTITY, |p| self.compute_global(p));
        let local = parent_xf.inverse().transform_point2(pos);
        if let Some(n) = self.nodes.get_mut(id) {
            n.transform.position = local;
            n.global.translation = pos;
        }
    }

    // ---------------------------------------------------------------- serialization

    /// Serializes `id` and its subtree.
    pub fn pack(&self, id: NodeId) -> Option<NodeData> {
        let n = self.nodes.get(id)?;
        let mut data = NodeData::from_node(n);
        data.children = n.children.iter().filter_map(|&c| self.pack(c)).collect();
        Some(data)
    }

    /// Builds a tree whose root is `data`.
    pub fn from_data(data: &NodeData) -> Self {
        let mut tree = SceneTree::with_root(data.to_node());
        let root = tree.root;
        for child in &data.children {
            tree.instantiate(root, child).expect("root exists");
        }
        tree.update_transforms();
        tree
    }

    /// Duplicates a subtree next to the original. Returns the copy's id.
    pub fn duplicate(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.parent(id)?;
        let data = self.pack(id)?;
        let copy = self.instantiate(parent, &data).ok()?;
        // Place the copy right after the original.
        let idx = self.children(parent).iter().position(|&c| c == id)? + 1;
        self.move_child(copy, idx);
        Some(copy)
    }

    /// Finds the active `WorldEnvironment`, if any.
    pub fn environment(&self) -> Option<&crate::kinds::WorldEnvironment> {
        self.all_nodes().into_iter().find_map(|id| match &self.nodes[id].kind {
            NodeKind::WorldEnvironment(env) => Some(env),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::*;
    use vd_core::vec2;

    fn n2d(name: &str) -> Node {
        Node::new(name, NodeKind::Node2D(Node2D::default()))
    }

    #[test]
    fn hierarchy_and_paths() {
        let mut t = SceneTree::new("Root");
        let root = t.root();
        let player = t.add_child(root, n2d("Player")).unwrap();
        let sprite = t.add_child(player, n2d("Sprite")).unwrap();
        let enemy = t.add_child(root, n2d("Enemy")).unwrap();
        let enemy2 = t.add_child(root, n2d("Enemy")).unwrap();
        assert_eq!(t.get(enemy2).unwrap().name, "Enemy2");
        assert_eq!(t.path_of(sprite), "/Root/Player/Sprite");
        assert_eq!(t.get_node(root, "Player/Sprite"), Some(sprite));
        assert_eq!(t.get_node(sprite, "../../Enemy"), Some(enemy));
        assert_eq!(t.get_node(sprite, "/Root/Enemy2"), Some(enemy2));
        assert_eq!(t.get_node(enemy, "%Sprite"), Some(sprite));
        assert_eq!(t.get_node(root, "Nope"), None);
        assert_eq!(t.preorder(root), vec![root, player, sprite, enemy, enemy2]);
    }

    #[test]
    fn transforms_propagate() {
        let mut t = SceneTree::new("Root");
        let root = t.root();
        let mut a = n2d("A");
        a.transform.position = vec2(100.0, 0.0);
        a.transform.scale = vec2(2.0, 2.0);
        let a = t.add_child(root, a).unwrap();
        let b = t.add_child(a, n2d("B").with_position(vec2(10.0, 5.0))).unwrap();
        t.update_transforms();
        assert_eq!(t.get(b).unwrap().global_position(), vec2(120.0, 10.0));
        t.set_global_position(b, vec2(0.0, 0.0));
        assert_eq!(t.get(b).unwrap().transform.position, vec2(-50.0, 0.0));
    }

    #[test]
    fn queue_free_and_stale_handles() {
        let mut t = SceneTree::new("Root");
        let root = t.root();
        let a = t.add_child(root, n2d("A")).unwrap();
        let b = t.add_child(a, n2d("B")).unwrap();
        t.queue_free(a);
        assert!(t.contains(b));
        let freed = t.flush_queued();
        assert_eq!(freed.len(), 2);
        assert!(!t.contains(a) && !t.contains(b));
        assert!(t.children(root).is_empty());
        let c = t.add_child(root, n2d("C")).unwrap();
        assert!(t.get(b).is_none(), "reused slot must not resurrect old handle");
        assert!(t.contains(c));
        assert_eq!(t.from_object(SceneTree::object_id(a)), None);
    }

    #[test]
    fn reparent_rejects_cycles_and_keeps_global() {
        let mut t = SceneTree::new("Root");
        let root = t.root();
        let a = t.add_child(root, n2d("A").with_position(vec2(50.0, 50.0))).unwrap();
        let b = t.add_child(root, n2d("B").with_position(vec2(10.0, 0.0))).unwrap();
        let a_child = t.add_child(a, n2d("AC")).unwrap();
        assert_eq!(t.reparent(a, a_child, None, true), Err(TreeError::Cycle));
        t.reparent(b, a, Some(0), true).unwrap();
        assert_eq!(t.children(a), &[b, a_child]);
        assert!((t.global_position(b) - vec2(10.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn pack_roundtrip_through_json() {
        let mut t = SceneTree::new("Level");
        let root = t.root();
        let mut s = Node::new("Ship", NodeKind::Shape2D(Shape2D { shape: ShapeType::Circle, ..Default::default() }));
        s.groups.push(StringName::new("player"));
        s.script = Some("player.vbp".into());
        let ship = t.add_child(root, s).unwrap();
        t.add_child(ship, Node::new("Glow", NodeKind::PointLight2D(PointLight2D::default()))).unwrap();
        let data = t.pack(root).unwrap();
        let json = serde_json::to_string_pretty(&data).unwrap();
        let back: NodeData = serde_json::from_str(&json).unwrap();
        let t2 = SceneTree::from_data(&back);
        assert_eq!(t2.len(), 3);
        let ship2 = t2.get_node(t2.root(), "Ship").unwrap();
        let n = t2.get(ship2).unwrap();
        assert!(matches!(&n.kind, NodeKind::Shape2D(s) if s.shape == ShapeType::Circle));
        assert_eq!(n.script.as_deref(), Some("player.vbp"));
        assert!(n.in_group(StringName::new("player")));
    }

    #[test]
    fn reflection_get_set() {
        let mut n = Node::new("L", NodeKind::Label(Label::default()));
        assert!(n.set("text", &"Hello".into()));
        assert_eq!(n.get("text").unwrap().to_string(), "Hello");
        n.set("align", &"Left".into());
        assert!(matches!(&n.kind, NodeKind::Label(l) if l.align == TextAlign::Left));
        n.set("score", &5.0.into());
        assert_eq!(n.get("score").unwrap().to_number(), 5.0);
        assert!(n.property_list().iter().any(|p| p.name == "font_size"));
    }
}
