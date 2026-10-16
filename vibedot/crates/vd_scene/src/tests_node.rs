//! Tests ported from Godot's `tests/scene/test_node.cpp` (same scenarios and
//! expectations; `memnew` → `tree.create`, `memdelete` → `tree.free`),
//! plus Vibedot-specific tests for notifications, signals and scene files.

use vd_core::{StringName, Variant, vec2};

use crate::kinds::*;
use crate::node::{Node, NodeId, NodeKind};
use crate::packed::{NodeData, SceneFile};
use crate::tree::{Notification, SceneTree, TreeError, TreeEvent, connect_flags};

fn plain() -> Node {
    Node::new("", NodeKind::Node(PlainNode::default()))
}

fn named(name: &str) -> Node {
    Node::new(name, NodeKind::Node(PlainNode::default()))
}

fn n2d(name: &str) -> Node {
    Node::new(name, NodeKind::Node2D(Node2D::default()))
}

// ---------------------------------------------------------------- "very simple scene tree"

fn simple() -> (SceneTree, NodeId) {
    let mut t = SceneTree::new();
    let node = t.create(plain());
    assert_eq!(t.get_child_count(t.root()), 0);
    assert_eq!(t.get_node_count(), 1);
    assert_eq!(t.get(node).unwrap().name, "");
    assert!(!t.is_inside_tree(node));
    assert_eq!(t.get_parent(node), None);
    assert!(t.get_path(node).is_empty());
    assert_eq!(t.get_child_count(node), 0);
    let root = t.root();
    t.add_child(root, node, false).unwrap();
    assert_eq!(t.get_child_count(root), 1);
    assert_eq!(t.get_node_count(), 2);
    assert_ne!(t.get(node).unwrap().name, "");
    assert!(t.is_inside_tree(node));
    assert_eq!(t.get_parent(node), Some(root));
    assert!(!t.get_path(node).is_empty());
    (t, node)
}

#[test]
fn simple_tree_first_child() {
    let (t, node) = simple();
    assert_eq!(t.get_child(t.root(), 0), Some(node));
}

#[test]
fn simple_tree_node_path() {
    let (mut t, node) = simple();
    let root = t.root();
    assert_eq!(t.get_node_or_null(root, &t.get_path(node)), Some(node));
    // The automatic name is "@Node@N", so "Node" does not resolve yet.
    assert_eq!(t.get_node_or_null(root, "Node"), None);
    assert_eq!(t.get_node_or_null(root, "/root/Node"), None);
    t.set_name(node, "Node");
    assert_eq!(t.get_node_or_null(root, &t.get_path(node)), Some(node));
    assert_eq!(t.get_node_or_null(root, "Node"), Some(node));
    assert_eq!(t.get_node_or_null(root, "/root/Node"), Some(node));
}

#[test]
fn simple_tree_group() {
    let (mut t, node) = simple();
    assert!(t.get_nodes_in_group("nodes").is_empty());
    t.get_mut(node).unwrap().add_to_group("nodes");
    assert_eq!(t.get_nodes_in_group("nodes"), vec![node]);
}

#[test]
fn simple_tree_find() {
    let (mut t, node) = simple();
    let root = t.root();
    assert_eq!(t.find_child(root, "Node", true), None);
    t.set_name(node, "Node");
    assert_eq!(t.find_child(root, "Node", true), Some(node));
}

#[test]
fn simple_tree_remove() {
    let (mut t, node) = simple();
    let root = t.root();
    t.remove_child(root, node).unwrap();
    assert_eq!(t.get_child_count(root), 0);
    assert_eq!(t.get_node_count(), 1);
    assert!(!t.is_inside_tree(node));
    assert_eq!(t.get_parent(node), None);
    assert!(t.get_path(node).is_empty());
    t.free(node).unwrap();
}

#[test]
fn simple_tree_move_and_reparent() {
    let (mut t, node) = simple();
    let root = t.root();
    t.move_child(root, node, 0);
    assert_eq!(t.get_child(root, 0), Some(node));
    assert!(t.is_inside_tree(node));
    t.reparent(node, root, true).unwrap();
    assert_eq!(t.get_child(root, 0), Some(node));
    assert!(t.is_inside_tree(node));
}

#[test]
fn simple_tree_duplicate() {
    let (mut t, node) = simple();
    t.set_name(node, "MyName");
    let dup = t.duplicate(node).unwrap();
    assert_ne!(node, dup);
    assert!(!t.is_inside_tree(dup));
    assert_eq!(t.get(dup).unwrap().name, t.get(node).unwrap().name);
    t.free(dup).unwrap();
}

// ---------------------------------------------------------------- "more complex simple scene tree"

struct Complex {
    t: SceneTree,
    root: NodeId,
    node1: NodeId,
    node2: NodeId,
    node1_1: NodeId,
}

fn complex() -> Complex {
    let mut t = SceneTree::new();
    let node1 = t.create(plain());
    let node2 = t.create(plain());
    let node1_1 = t.create(plain());
    let root = t.root();
    t.add_child(root, node1, false).unwrap();
    t.add_child(root, node2, false).unwrap();
    t.add_child(node1, node1_1, false).unwrap();
    assert!(t.is_inside_tree(node1_1));
    assert_eq!(t.get_parent(node1_1), Some(node1));
    assert_eq!(t.get_child_count(node1), 1);
    assert!(t.is_ancestor_of(root, node1));
    assert!(t.is_ancestor_of(root, node2));
    assert!(t.is_ancestor_of(root, node1_1));
    assert!(!t.is_ancestor_of(node1, node1));
    assert!(t.is_ancestor_of(node1, node1_1));
    assert!(!t.is_ancestor_of(node2, node1_1));
    assert!(!t.is_ancestor_of(node2, node1));
    assert_eq!(t.get_child_count(root), 2);
    assert_eq!(t.get_node_count(), 4);
    Complex { t, root, node1, node2, node1_1 }
}

#[test]
fn complex_get_child() {
    let c = complex();
    assert_eq!(c.t.get_child(c.root, 0), Some(c.node1));
    assert_eq!(c.t.get_child(c.root, 1), Some(c.node2));
    assert_eq!(c.t.get_child(c.node1, 0), Some(c.node1_1));
    assert_eq!(c.t.get_child(c.root, -1), Some(c.node2), "negative index counts from the end");
}

#[test]
fn complex_remove_removes_children_from_tree() {
    let mut c = complex();
    c.t.remove_child(c.root, c.node1).unwrap();
    assert_eq!(c.t.get_child_count(c.node1), 1);
    assert_eq!(c.t.get_child_count(c.root), 1);
    assert_eq!(c.t.get_node_count(), 2);
    assert_eq!(c.t.get_child(c.root, 0), Some(c.node2));
}

#[test]
fn complex_remove_child_keeps_parent() {
    let mut c = complex();
    c.t.remove_child(c.node1, c.node1_1).unwrap();
    assert_eq!(c.t.get_parent(c.node1_1), None);
    assert_eq!(c.t.get_child_count(c.node1), 0);
    assert_eq!(c.t.get_node_count(), 3);
}

#[test]
fn complex_move_child_back_and_front() {
    let mut c = complex();
    c.t.move_child(c.root, c.node1, 1);
    assert_eq!(c.t.get_child(c.root, 0), Some(c.node2));
    assert_eq!(c.t.get_child(c.root, 1), Some(c.node1));
    let mut c = complex();
    c.t.move_child(c.root, c.node2, 0);
    assert_eq!(c.t.get_child(c.root, 0), Some(c.node2));
    assert_eq!(c.t.get_child(c.root, 1), Some(c.node1));
}

#[test]
fn complex_reparent_remove_add_and_reparent() {
    let mut c = complex();
    assert_eq!(c.t.get_child_count(c.node2), 0);
    c.t.remove_child(c.node1, c.node1_1).unwrap();
    assert_eq!(c.t.get_child_count(c.node1), 0);
    assert_eq!(c.t.get_parent(c.node1_1), None);
    c.t.add_child(c.node2, c.node1_1, false).unwrap();
    assert_eq!(c.t.get_child_count(c.node2), 1);
    assert_eq!(c.t.get_parent(c.node1_1), Some(c.node2));
    assert_eq!(c.t.get_child(c.node2, 0), Some(c.node1_1));
    assert_eq!(c.t.get_child_count(c.root), 2);
    assert_eq!(c.t.get_node_count(), 4);

    let mut c = complex();
    c.t.reparent(c.node1_1, c.node2, true).unwrap();
    assert_eq!(c.t.get_child_count(c.node1), 0);
    assert_eq!(c.t.get_child_count(c.node2), 1);
    assert_eq!(c.t.get_parent(c.node1_1), Some(c.node2));
    assert_eq!(c.t.get_node_count(), 4);
}

#[test]
fn complex_find_child_and_children() {
    let mut c = complex();
    assert_eq!(c.t.find_child(c.root, "NestedNode", true), None);
    assert!(c.t.find_children(c.root, "NestedNode", "", true).is_empty());
    c.t.set_name(c.node1, "Node1");
    c.t.set_name(c.node2, "Node2");
    c.t.set_name(c.node1_1, "NestedNode");
    assert_eq!(c.t.find_child(c.root, "NestedNode", true), Some(c.node1_1));
    assert_eq!(c.t.find_children(c.root, "NestedNode", "", true), vec![c.node1_1]);
    // First node that matches with the name is node1.
    assert_eq!(c.t.find_child(c.root, "Node?", true), Some(c.node1));
    assert_eq!(c.t.find_children(c.root, "Node?", "", true), vec![c.node1, c.node2]);
    c.t.move_child(c.root, c.node2, 0);
    assert_eq!(c.t.find_child(c.root, "Node?", true), Some(c.node2));
    assert_eq!(c.t.find_children(c.root, "Node?", "", true), vec![c.node2, c.node1]);
}

#[test]
fn complex_node_paths() {
    let mut c = complex();
    for n in [c.node1, c.node2, c.node1_1] {
        assert_eq!(c.t.get_node_or_null(c.root, &c.t.get_path(n)), Some(n));
    }
    c.t.set_name(c.node1, "Node1");
    c.t.set_name(c.node1_1, "NestedNode");
    assert_eq!(c.t.get_node_or_null(c.node1, "NestedNode"), Some(c.node1_1));
    assert_eq!(c.t.get_node_or_null(c.root, "/root/Node1/NestedNode"), Some(c.node1_1));
    assert_eq!(c.t.get_node_or_null(c.root, "Node1/NestedNode"), Some(c.node1_1));
}

#[test]
fn complex_groups() {
    let mut c = complex();
    assert!(c.t.get_nodes_in_group("nodes").is_empty());
    assert!(c.t.get_nodes_in_group("other_nodes").is_empty());
    c.t.get_mut(c.node1).unwrap().add_to_group("nodes");
    c.t.get_mut(c.node2).unwrap().add_to_group("other_nodes");
    c.t.get_mut(c.node1_1).unwrap().add_to_group("nodes");
    c.t.get_mut(c.node1_1).unwrap().add_to_group("other_nodes");
    assert_eq!(c.t.get_nodes_in_group("nodes"), vec![c.node1, c.node1_1]);
    assert_eq!(c.t.get_nodes_in_group("other_nodes"), vec![c.node1_1, c.node2]);
    c.t.get_mut(c.node1).unwrap().remove_from_group("nodes");
    assert_eq!(c.t.get_nodes_in_group("nodes"), vec![c.node1_1]);
}

#[test]
fn complex_add_sibling() {
    let mut c = complex();
    c.t.remove_child(c.node1, c.node1_1).unwrap();
    c.t.add_sibling(c.node1, c.node1_1, false).unwrap();
    assert_eq!(c.t.get_child_count(c.root), 3);
    assert_eq!(c.t.get_node_count(), 4);
    assert_eq!(c.t.get_child(c.root, 0), Some(c.node1));
    assert_eq!(c.t.get_child(c.root, 1), Some(c.node1_1));
    assert_eq!(c.t.get_child(c.root, 2), Some(c.node2));
}

#[test]
fn complex_replace_by() {
    let mut c = complex();
    c.t.remove_child(c.root, c.node2).unwrap();
    c.t.replace_by(c.node1, c.node2, false).unwrap();
    assert_eq!(c.t.get_child_count(c.root), 1);
    assert_eq!(c.t.get_node_count(), 3);
    assert!(!c.t.is_inside_tree(c.node1));
    assert!(c.t.is_inside_tree(c.node2));
    assert_eq!(c.t.get_parent(c.node1), None);
    assert_eq!(c.t.get_parent(c.node2), Some(c.root));
    assert_eq!(c.t.get_child_count(c.node2), 1);
    assert_eq!(c.t.get_child(c.node2, 0), Some(c.node1_1));

    let mut c = complex();
    c.t.remove_child(c.root, c.node2).unwrap();
    c.t.get_mut(c.node1).unwrap().add_to_group("nodes");
    c.t.replace_by(c.node1, c.node2, true).unwrap();
    assert_eq!(c.t.get_nodes_in_group("nodes"), vec![c.node2]);
}

#[test]
fn complex_duplicate_children() {
    let mut c = complex();
    c.t.set_name(c.node1, "MyName1");
    c.t.set_name(c.node1_1, "MyName1_1");
    let d1 = c.t.duplicate(c.node1).unwrap();
    assert_eq!(c.t.get_child_count(d1), c.t.get_child_count(c.node1));
    let d1_1 = c.t.get_child(d1, 0).unwrap();
    assert_eq!(c.t.get_child_count(d1_1), c.t.get_child_count(c.node1_1));
    assert_eq!(c.t.get(d1).unwrap().name, "MyName1");
    assert_eq!(c.t.get(d1_1).unwrap().name, "MyName1_1");
    assert!(!c.t.is_inside_tree(d1));
    assert!(!c.t.is_inside_tree(d1_1));
    c.t.free(d1).unwrap();
    assert!(!c.t.contains(d1_1));
}

// ---------------------------------------------------------------- processing

fn processing_node(t: &mut SceneTree) -> NodeId {
    let n = t.create(plain());
    let root = t.root();
    t.add_child(root, n, false).unwrap();
    n
}

#[test]
fn processing_flags() {
    let mut t = SceneTree::new();
    let node = processing_node(&mut t);
    assert!(t.process().is_empty() && t.physics_process().is_empty());
    t.get_mut(node).unwrap().set_process(true);
    assert_eq!(t.process(), vec![node]);
    assert!(t.physics_process().is_empty());
    assert!(t.internal_process().is_empty());
    t.get_mut(node).unwrap().set_physics_process(true);
    t.get_mut(node).unwrap().set_process_internal(true);
    t.get_mut(node).unwrap().set_physics_process_internal(true);
    assert_eq!(t.physics_process(), vec![node]);
    assert_eq!(t.internal_process(), vec![node]);
    assert_eq!(t.internal_physics_process(), vec![node]);
    let n = t.get_mut(node).unwrap();
    n.set_process(false);
    n.set_physics_process(false);
    n.set_process_internal(false);
    n.set_physics_process_internal(false);
    assert!(t.process().is_empty() && t.physics_process().is_empty());
}

#[test]
fn process_priority_order() {
    let mut t = SceneTree::new();
    let nodes: Vec<NodeId> = (0..4).map(|_| processing_node(&mut t)).collect();
    for (&n, p) in nodes.iter().zip([20, 10, 40, 30]) {
        let node = t.get_mut(n).unwrap();
        node.set_process(true);
        node.set_process_priority(p);
        node.set_physics_process(true);
        node.set_physics_process_priority(p);
    }
    let expected = vec![nodes[1], nodes[0], nodes[3], nodes[2]];
    assert_eq!(t.process(), expected);
    assert_eq!(t.physics_process(), expected);
}

#[test]
fn pause_and_process_modes() {
    use crate::node::ProcessMode;
    let mut t = SceneTree::new();
    let game = processing_node(&mut t);
    let menu = processing_node(&mut t);
    let child = t.add_new_child(game, plain()).unwrap();
    for n in [game, menu, child] {
        t.get_mut(n).unwrap().set_process(true);
    }
    t.get_mut(menu).unwrap().process_mode = ProcessMode::WhenPaused;
    assert_eq!(t.process(), vec![game, child]);
    t.set_pause(true);
    assert_eq!(t.process(), vec![menu]);
    t.get_mut(game).unwrap().process_mode = ProcessMode::Always;
    assert_eq!(t.process(), vec![game, child, menu], "child inherits Always");
    t.get_mut(child).unwrap().process_mode = ProcessMode::Disabled;
    assert_eq!(t.process(), vec![game, menu]);
}

// ---------------------------------------------------------------- naming

#[test]
fn readable_and_automatic_names() {
    let mut t = SceneTree::new();
    let root = t.root();
    let a = t.add_new_child(root, named("Enemy")).unwrap();
    let b = t.add_new_child(root, named("Enemy")).unwrap();
    let c = t.add_new_child(root, named("Enemy")).unwrap();
    assert_eq!(t.get(a).unwrap().name, "Enemy");
    assert_eq!(t.get(b).unwrap().name, "Enemy2");
    assert_eq!(t.get(c).unwrap().name, "Enemy3");
    let d = t.add_new_child(root, named("Enemy9")).unwrap();
    let e = t.add_new_child(root, named("Enemy9")).unwrap();
    assert_eq!(t.get(d).unwrap().name, "Enemy9");
    assert_eq!(t.get(e).unwrap().name, "Enemy10");
    // Non-readable mode: conflicts become @Class@N.
    let f = t.create(named("Enemy"));
    t.add_child(root, f, false).unwrap();
    assert!(t.get(f).unwrap().name.starts_with("@Node@"), "{}", t.get(f).unwrap().name);
    // Invalid characters are replaced.
    t.set_name(a, "Bad/Name.%");
    assert_eq!(t.get(a).unwrap().name, "Bad_Name__");
    // Empty readable name uses the class name.
    let g = t.add_new_child(root, Node::new("", NodeKind::Sprite2D(Sprite2D::default()))).unwrap();
    assert_eq!(t.get(g).unwrap().name, "Sprite2D");
}

#[test]
fn add_child_errors() {
    let mut t = SceneTree::new();
    let root = t.root();
    let a = t.add_new_child(root, named("A")).unwrap();
    let b = t.add_new_child(a, named("B")).unwrap();
    assert!(matches!(t.add_child(b, a, false), Err(TreeError::Cycle(_))));
    assert!(matches!(t.add_child(root, b, false), Err(TreeError::HasParent(_))));
    assert_eq!(t.free(root), Err(TreeError::Root));
}

#[test]
fn paths_and_unique_names() {
    let mut t = SceneTree::new();
    let root = t.root();
    let main = t.add_new_child(root, n2d("Main")).unwrap();
    let player = t.add_new_child(main, n2d("Player")).unwrap();
    let hud = t.add_new_child(main, n2d("HUD")).unwrap();
    let score = t.add_new_child(hud, named("Score")).unwrap();
    assert_eq!(t.get_path(score), "/root/Main/HUD/Score");
    assert_eq!(t.get_path_to(player, score), "../HUD/Score");
    assert_eq!(t.get_path_to(score, player), "../../Player");
    assert_eq!(t.get_path_to(main, score), "HUD/Score");
    assert_eq!(t.get_path_to(score, score), ".");
    assert_eq!(t.get_node_or_null(player, "%Score"), None, "not unique yet");
    t.get_mut(score).unwrap().unique_name_in_owner = true;
    assert_eq!(t.get_node_or_null(player, "%Score"), Some(score));
    assert_eq!(t.get_node_or_null(player, "../HUD/Score"), Some(score));
    assert_eq!(t.get_node_or_null(score, "../.."), Some(main));
}

// ---------------------------------------------------------------- notifications & signals

fn notifications(events: &[TreeEvent]) -> Vec<(NodeId, Notification)> {
    events
        .iter()
        .filter_map(|e| match e {
            TreeEvent::Notification(id, n) => Some((*id, *n)),
            TreeEvent::Call { .. } => None,
        })
        .collect()
}

#[test]
fn notification_order_matches_godot() {
    let mut t = SceneTree::new();
    let root = t.root();
    // Build an orphan branch: A { B { C }, D }
    let a = t.create(named("A"));
    let b = t.add_new_child(a, named("B")).unwrap();
    let c = t.add_new_child(b, named("C")).unwrap();
    let d = t.add_new_child(a, named("D")).unwrap();
    assert!(t.take_events().is_empty(), "orphans receive no notifications");
    t.add_child(root, a, true).unwrap();
    use Notification::*;
    assert_eq!(
        notifications(&t.take_events()),
        vec![(a, EnterTree), (b, EnterTree), (c, EnterTree), (d, EnterTree), (c, Ready), (b, Ready), (d, Ready), (a, Ready)]
    );
    t.remove_child(root, a).unwrap();
    assert_eq!(notifications(&t.take_events()), vec![(d, ExitTree), (c, ExitTree), (b, ExitTree), (a, ExitTree)]);
    // Ready fires only once unless requested again.
    t.add_child(root, a, true).unwrap();
    assert!(!notifications(&t.take_events()).iter().any(|(_, n)| *n == Ready));
    t.remove_child(root, a).unwrap();
    t.get_mut(b).unwrap().request_ready();
    t.take_events();
    t.add_child(root, a, true).unwrap();
    let ready: Vec<_> = notifications(&t.take_events()).into_iter().filter(|(_, n)| *n == Ready).collect();
    assert_eq!(ready, vec![(b, Ready)]);
}

#[test]
fn signals_connect_emit_one_shot_and_binds() {
    let mut t = SceneTree::new();
    let root = t.root();
    let timer = t.add_new_child(root, Node::new("Timer", NodeKind::Timer(Timer::default()))).unwrap();
    let player = t.add_new_child(root, n2d("Player")).unwrap();
    t.take_events();
    t.connect(timer, "timeout", player, "_on_timeout", 0).unwrap();
    assert_eq!(t.connect(timer, "timeout", player, "_on_timeout", 0), Err(TreeError::AlreadyConnected("timeout".into())));
    t.connect_with(timer, "timeout", player, "_once", connect_flags::ONE_SHOT, vec![Variant::from(7.0)]).unwrap();
    assert!(t.is_connected(timer, "timeout", player, "_on_timeout"));
    assert_eq!(t.emit_signal(timer, "timeout", &[]), 2);
    assert_eq!(t.emit_signal(timer, "timeout", &[]), 1, "one-shot connection removed");
    let calls: Vec<(StringName, Vec<Variant>)> = t
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            TreeEvent::Call { method, args, target, .. } if target == player => Some((method, args)),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1], (StringName::new("_once"), vec![Variant::from(7.0)]));
    t.disconnect(timer, "timeout", player, "_on_timeout").unwrap();
    assert_eq!(t.emit_signal(timer, "timeout", &[]), 0);
    // Freeing the target drops connections to it.
    t.connect(timer, "timeout", player, "_on_timeout", 0).unwrap();
    t.free(player).unwrap();
    assert_eq!(t.emit_signal(timer, "timeout", &[]), 0);
}

#[test]
fn builtin_signals_fire() {
    let mut t = SceneTree::new();
    let root = t.root();
    let parent = t.add_new_child(root, named("Parent")).unwrap();
    let watcher = t.add_new_child(root, named("Watcher")).unwrap();
    t.connect(parent, "child_entered_tree", watcher, "_on_child", 0).unwrap();
    t.connect(parent, "renamed", watcher, "_on_renamed", 0).unwrap();
    t.take_events();
    let child = t.add_new_child(parent, named("Kid")).unwrap();
    t.set_name(parent, "Renamed");
    let methods: Vec<String> = t
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            TreeEvent::Call { method, args, .. } => Some(format!("{method}{:?}", args.len())),
            _ => None,
        })
        .collect();
    assert_eq!(methods, vec!["_on_child1", "_on_renamed0"]);
    let _ = child;
}

#[test]
fn queue_free_and_stale_handles() {
    let mut t = SceneTree::new();
    let root = t.root();
    let a = t.add_new_child(root, n2d("A")).unwrap();
    let b = t.add_new_child(a, n2d("B")).unwrap();
    t.queue_free(a);
    assert!(t.contains(b));
    assert_eq!(t.flush_queued().len(), 2);
    assert!(!t.contains(a) && !t.contains(b));
    let c = t.add_new_child(root, n2d("C")).unwrap();
    assert!(t.get(b).is_none(), "reused slot must not resurrect old handle");
    assert!(t.contains(c));
    assert_eq!(t.from_object(SceneTree::object_id(a)), None);
}

#[test]
fn transforms_propagate_and_reparent_keeps_global() {
    let mut t = SceneTree::new();
    let root = t.root();
    let mut a = n2d("A");
    a.transform.position = vec2(100.0, 0.0);
    a.transform.scale = vec2(2.0, 2.0);
    let a = t.add_new_child(root, a).unwrap();
    let b = t.add_new_child(a, n2d("B").with_position(vec2(10.0, 5.0))).unwrap();
    t.update_transforms();
    assert_eq!(t.get(b).unwrap().global_position(), vec2(120.0, 10.0));
    t.set_global_position(b, vec2(0.0, 0.0));
    assert_eq!(t.get(b).unwrap().transform.position, vec2(-50.0, 0.0));
    let c = t.add_new_child(root, n2d("C").with_position(vec2(10.0, 0.0))).unwrap();
    t.reparent_at(c, a, Some(0), true).unwrap();
    assert_eq!(t.get_children(a), &[c, b]);
    assert!((t.global_position(c) - vec2(10.0, 0.0)).length() < 1e-4);
}

#[test]
fn scene_file_roundtrip_with_connections() {
    let mut t = SceneTree::new();
    let root = t.root();
    let level = t.add_new_child(root, n2d("Level")).unwrap();
    t.set_current_scene(Some(level));
    let mut ship = Node::new("Ship", NodeKind::Shape2D(Shape2D { shape: ShapeType::Circle, ..Default::default() }));
    ship.add_to_group("player");
    ship.script = Some("player.vbp".into());
    ship.unique_name_in_owner = true;
    let ship = t.add_new_child(level, ship).unwrap();
    let timer = t.add_new_child(level, Node::new("Timer", NodeKind::Timer(Timer::default()))).unwrap();
    t.add_new_child(ship, Node::new("Glow", NodeKind::PointLight2D(PointLight2D::default()))).unwrap();
    t.connect(timer, "timeout", ship, "fire", connect_flags::PERSIST).unwrap();
    t.connect(timer, "timeout", level, "not_saved", 0).unwrap();

    let json = SceneFile::from_tree(&t).to_json();
    let back = SceneFile::from_json(&json).unwrap();
    assert_eq!(back.root.connections.len(), 1);
    let t2 = back.to_tree();
    let scene = t2.current_scene().unwrap();
    assert_eq!(t2.get_path(scene), "/root/Level");
    let ship2 = t2.get_node_or_null(scene, "%Ship").unwrap();
    let n = t2.get(ship2).unwrap();
    assert!(matches!(&n.kind, NodeKind::Shape2D(s) if s.shape == ShapeType::Circle));
    assert_eq!(n.script.as_deref(), Some("player.vbp"));
    assert!(n.is_in_group("player"));
    let timer2 = t2.get_node_or_null(scene, "Timer").unwrap();
    assert!(t2.is_connected(timer2, "timeout", ship2, "fire"));
    assert!(!t2.is_connected(timer2, "timeout", scene, "not_saved"));
    let _: NodeData = back.root;
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
    n.set("process_mode", &"Always".into());
    assert_eq!(n.process_mode, crate::node::ProcessMode::Always);
    assert!(n.property_list().iter().any(|p| p.name == "font_size"));
}
