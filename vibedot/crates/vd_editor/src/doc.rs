//! Editable documents (scenes and blueprints) with snapshot-based undo.
//!
//! Instead of a command pattern, every edit marks the document as touched;
//! once the mouse is released the document is serialized and compared with
//! the last snapshot. Continuous gestures (dragging a node, scrubbing a
//! value) therefore collapse into a single undo step automatically.

use std::collections::BTreeSet;
use std::path::Path;

use vd_blueprint::{BpNodeId, Blueprint, CompileError};
use vd_core::Vec2;
use vd_scene::{NodeId, SceneFile, SceneTree};

/// Generic snapshot history.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<String>,
    redo: Vec<String>,
    current: String,
}

impl History {
    pub fn new(initial: String) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), current: initial }
    }

    /// Records `now` if it differs from the current snapshot. Returns true
    /// if a new step was recorded.
    pub fn record(&mut self, now: String) -> bool {
        if now == self.current {
            return false;
        }
        self.undo.push(std::mem::replace(&mut self.current, now));
        self.redo.clear();
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        true
    }

    pub fn undo(&mut self) -> Option<&str> {
        let prev = self.undo.pop()?;
        self.redo.push(std::mem::replace(&mut self.current, prev));
        Some(&self.current)
    }

    pub fn redo(&mut self) -> Option<&str> {
        let next = self.redo.pop()?;
        self.undo.push(std::mem::replace(&mut self.current, next));
        Some(&self.current)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

pub struct SceneDoc {
    /// Project-relative path (`scenes/main.vscn`).
    pub path: String,
    pub tree: SceneTree,
    /// Selected nodes; the last one is the primary selection.
    pub selection: Vec<NodeId>,
    pub history: History,
    pub touched: bool,
    pub dirty: bool,
    /// Nodes expanded in the scene dock (by path, survives undo).
    pub expanded: BTreeSet<String>,
}

impl SceneDoc {
    pub fn new(path: &str, tree: SceneTree) -> Self {
        let snap = SceneFile::from_tree(&tree).to_json();
        let mut expanded = BTreeSet::new();
        if let Some(root) = tree.current_scene() {
            expanded.insert(tree.path_of(root));
        }
        Self { path: path.to_owned(), tree, selection: Vec::new(), history: History::new(snap), touched: false, dirty: false, expanded }
    }

    pub fn load(project_dir: &Path, path: &str) -> Result<Self, String> {
        let file = SceneFile::load(&project_dir.join(path)).map_err(|e| format!("{path}: {e}"))?;
        Ok(Self::new(path, file.to_tree()))
    }

    pub fn root(&self) -> NodeId {
        self.tree.scene_root()
    }

    pub fn primary(&self) -> Option<NodeId> {
        self.selection.last().copied().filter(|&id| self.tree.contains(id))
    }

    pub fn select(&mut self, id: NodeId, additive: bool) {
        if additive {
            if let Some(i) = self.selection.iter().position(|&s| s == id) {
                self.selection.remove(i);
            } else {
                self.selection.push(id);
            }
        } else {
            self.selection = vec![id];
        }
        // Make sure the selection is visible in the dock.
        let mut cur = self.tree.get_parent(id);
        while let Some(p) = cur {
            self.expanded.insert(self.tree.path_of(p));
            cur = self.tree.get_parent(p);
        }
    }

    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selection.contains(&id)
    }

    pub fn snapshot(&self) -> String {
        SceneFile::from_tree(&self.tree).to_json()
    }

    /// Call once per frame; commits touched edits when no gesture is active.
    pub fn commit(&mut self, gesture_active: bool) {
        if self.touched && !gesture_active {
            self.touched = false;
            self.tree.flush_queued();
            if self.history.record(self.snapshot()) {
                self.dirty = true;
            }
        }
    }

    fn restore(&mut self, json: &str) {
        let sel_paths: Vec<String> = self.selection.iter().map(|&id| self.tree.path_of(id)).collect();
        if let Ok(file) = SceneFile::from_json(json) {
            self.tree = file.to_tree();
            let root = self.tree.root();
            self.selection = sel_paths.iter().filter_map(|p| self.tree.get_node_or_null(root, p)).collect();
            self.dirty = true;
        }
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.history.undo().map(str::to_owned) {
            self.restore(&s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.history.redo().map(str::to_owned) {
            self.restore(&s);
        }
    }

    pub fn save(&mut self, project_dir: &Path) -> Result<(), String> {
        SceneFile::from_tree(&self.tree).save(&project_dir.join(&self.path)).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }

}

/// Blueprint editor view state.
#[derive(Clone, Debug)]
pub struct GraphView {
    /// Screen offset of the graph origin (points).
    pub pan: Vec2,
    pub zoom: f32,
    pub initialized: bool,
}

impl Default for GraphView {
    fn default() -> Self {
        Self { pan: Vec2::new(40.0, 40.0), zoom: 1.0, initialized: false }
    }
}

pub struct BpDoc {
    pub path: String,
    pub bp: Blueprint,
    pub history: History,
    pub touched: bool,
    pub dirty: bool,
    pub view: GraphView,
    pub selection: BTreeSet<BpNodeId>,
    pub errors: Vec<CompileError>,
    pub disassembly: String,
}

impl BpDoc {
    pub fn new(path: &str, bp: Blueprint) -> Self {
        let mut d = Self {
            path: path.to_owned(),
            history: History::new(bp.to_json()),
            bp,
            touched: false,
            dirty: false,
            view: GraphView::default(),
            selection: BTreeSet::new(),
            errors: Vec::new(),
            disassembly: String::new(),
        };
        d.recompile();
        d
    }

    pub fn load(project_dir: &Path, path: &str) -> Result<Self, String> {
        let bp = Blueprint::load(&project_dir.join(path)).map_err(|e| format!("{path}: {e}"))?;
        Ok(Self::new(path, bp))
    }

    pub fn recompile(&mut self) {
        match vd_blueprint::compile(&self.bp) {
            Ok(p) => {
                self.errors.clear();
                self.disassembly = p.disassemble();
            }
            Err(e) => {
                self.errors = e;
                self.disassembly.clear();
            }
        }
    }

    pub fn commit(&mut self, gesture_active: bool) {
        if self.touched && !gesture_active {
            self.touched = false;
            if self.history.record(self.bp.to_json()) {
                self.dirty = true;
                self.recompile();
            }
        }
    }

    fn restore(&mut self, json: &str) {
        if let Ok(bp) = Blueprint::from_json(json) {
            self.bp = bp;
            self.selection.retain(|id| self.bp.node(*id).is_some());
            self.dirty = true;
            self.recompile();
        }
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.history.undo().map(str::to_owned) {
            self.restore(&s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.history.redo().map(str::to_owned) {
            self.restore(&s);
        }
    }

    pub fn save(&mut self, project_dir: &Path) -> Result<(), String> {
        self.bp.save(&project_dir.join(&self.path)).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn has_error(&self, id: BpNodeId) -> Option<&str> {
        self.errors.iter().find(|e| e.node == Some(id)).map(|e| e.message.as_str())
    }
}

/// A fresh blueprint with the two most common events placed.
pub fn starter_blueprint() -> Blueprint {
    let mut bp = Blueprint::new();
    let ready = bp.add("event.ready", (0.0, 0.0), &[]);
    let hello = bp.add("debug.print", (300.0, 0.0), &[("value", vd_core::Variant::str("Hello from a blueprint!"))]);
    bp.connect(vd_blueprint::PinRef::new(ready, "then"), vd_blueprint::PinRef::new(hello, "exec")).expect("valid");
    bp.add("event.process", (0.0, 200.0), &[]);
    bp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_coalesces_and_roundtrips() {
        let mut h = History::new("a".into());
        assert!(!h.record("a".into()));
        assert!(h.record("b".into()));
        assert!(h.record("c".into()));
        assert_eq!(h.undo(), Some("b"));
        assert_eq!(h.undo(), Some("a"));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some("b"));
        assert!(h.record("d".into()));
        assert!(!h.can_redo(), "new edit clears redo");
    }

    #[test]
    fn scene_doc_undo_restores_tree_and_selection() {
        let files = vd_engine::templates::empty_project("T");
        let json = String::from_utf8(files.iter().find(|(p, _)| p == "scenes/main.vscn").unwrap().1.clone()).unwrap();
        let mut doc = SceneDoc::new("scenes/main.vscn", SceneFile::from_json(&json).unwrap().to_tree());
        let root = doc.root();
        let hello = doc.tree.get_node(root, "Hello").unwrap();
        doc.select(hello, false);
        doc.tree.get_mut(hello).unwrap().transform.position.x = 123.0;
        doc.touched = true;
        doc.commit(true);
        assert!(!doc.history.can_undo(), "no commit during a gesture");
        doc.commit(false);
        assert!(doc.history.can_undo() && doc.dirty);
        doc.undo();
        let hello = doc.primary().expect("selection restored by path");
        assert_eq!(doc.tree.get(hello).unwrap().transform.position.x, 0.0);
        doc.redo();
        let hello = doc.primary().unwrap();
        assert_eq!(doc.tree.get(hello).unwrap().transform.position.x, 123.0);
    }
}
