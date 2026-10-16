//! End-to-end tests: build graphs in code, compile, run against a real tree.

use std::sync::Arc;

use vd_core::{StringName, VType, Variant, vec2};
use vd_scene::{Label, Node, NodeKind, SceneTree, Node2D};

use crate::defs::EventKind;
use crate::graph::{Blueprint, PinRef};
use crate::vm::{Instance, NullHost};
use crate::compile;

fn link(bp: &mut Blueprint, a: crate::BpNodeId, ap: &str, b: crate::BpNodeId, bp_: &str) {
    bp.connect(PinRef::new(a, ap), PinRef::new(b, bp_)).unwrap_or_else(|e| panic!("{ap} -> {bp_}: {e}"));
}

fn setup(bp: &Blueprint) -> (SceneTree, Instance, NullHost) {
    let program = Arc::new(compile(bp).unwrap_or_else(|e| panic!("compile errors: {e:?}")));
    let mut tree = SceneTree::new("Root");
    let id = tree.add_child(tree.root(), Node::new("Hero", NodeKind::Node2D(Node2D::default()))).unwrap();
    let inst = Instance::new(program, id, |_| None);
    (tree, inst, NullHost::default())
}

fn fire(inst: &mut Instance, tree: &mut SceneTree, host: &mut NullHost, kind: EventKind) {
    inst.trigger(kind, StringName::EMPTY, &[Variant::Number(0.016)], tree, host).unwrap();
}

#[test]
fn for_loop_sums_into_variable() {
    // On Ready -> For 1..10 { total += index } -> Print(total)
    let mut bp = Blueprint::new();
    bp.add_variable("total", VType::Number, Variant::Number(0.0));
    let ev = bp.add("event.ready", (0.0, 0.0), &[]);
    let lp = bp.add("flow.for", (200.0, 0.0), &[("first", 1.0.into()), ("last", 10.0.into())]);
    let add = bp.add("var.add", (400.0, 0.0), &[("var", "total".into())]);
    let get = bp.add("var.get", (400.0, 200.0), &[("var", "total".into())]);
    let pr = bp.add("debug.print", (600.0, 0.0), &[]);
    link(&mut bp, ev, "then", lp, "exec");
    link(&mut bp, lp, "body", add, "exec");
    link(&mut bp, lp, "index", add, "amount");
    link(&mut bp, lp, "completed", pr, "exec");
    link(&mut bp, get, "value", pr, "value");
    let (mut tree, mut inst, mut host) = setup(&bp);
    fire(&mut inst, &mut tree, &mut host, EventKind::Ready);
    assert_eq!(host.log, vec!["[Hero] 55"]);
    assert_eq!(inst.var("total"), Some(&Variant::Number(55.0)));
}

#[test]
fn branch_and_comparison() {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.process", (0.0, 0.0), &[]);
    let gt = bp.add("cmp.gt", (0.0, 100.0), &[("b", 0.01.into())]);
    let br = bp.add("flow.branch", (200.0, 0.0), &[]);
    let yes = bp.add("debug.print", (400.0, 0.0), &[("value", "slow frame".into())]);
    let no = bp.add("debug.print", (400.0, 100.0), &[("value", "fast frame".into())]);
    link(&mut bp, ev, "delta", gt, "a");
    link(&mut bp, gt, "result", br, "condition");
    link(&mut bp, ev, "then", br, "exec");
    link(&mut bp, br, "true", yes, "exec");
    link(&mut bp, br, "false", no, "exec");
    let (mut tree, mut inst, mut host) = setup(&bp);
    inst.trigger(EventKind::Process, StringName::EMPTY, &[Variant::Number(0.016)], &mut tree, &mut host).unwrap();
    inst.trigger(EventKind::Process, StringName::EMPTY, &[Variant::Number(0.001)], &mut tree, &mut host).unwrap();
    assert_eq!(host.log, vec!["[Hero] slow frame", "[Hero] fast frame"]);
}

#[test]
fn moves_self_and_formats_text() {
    // On Process: Move By (velocity * delta); Set Text on a Label child.
    let mut bp = Blueprint::new();
    let ev = bp.add("event.process", (0.0, 0.0), &[]);
    let mul = bp.add("math.mul", (0.0, 100.0), &[]);
    let vel = bp.add("lit.vec2", (0.0, 200.0), &[("value", Variant::Vec2(vec2(100.0, 0.0)))]);
    let mv = bp.add("node.translate", (200.0, 0.0), &[]);
    let lbl = bp.add("node.get_node", (200.0, 200.0), &[("path", "Score".into())]);
    let fmt = bp.add("str.format", (200.0, 300.0), &[("template", "x = {0}".into())]);
    let pos = bp.add("node.get_position", (100.0, 300.0), &[]);
    let brk = bp.add("vec.break", (150.0, 300.0), &[]);
    let st = bp.add("node.set_text", (400.0, 0.0), &[]);
    link(&mut bp, ev, "then", mv, "exec");
    link(&mut bp, vel, "out", mul, "a");
    link(&mut bp, ev, "delta", mul, "b");
    link(&mut bp, mul, "result", mv, "offset");
    link(&mut bp, mv, "then", st, "exec");
    link(&mut bp, lbl, "node", st, "node");
    link(&mut bp, pos, "position", brk, "vector");
    link(&mut bp, brk, "x", fmt, "0");
    link(&mut bp, fmt, "result", st, "text");
    let (mut tree, mut inst, mut host) = setup(&bp);
    let hero = inst.node;
    let label = tree.add_child(hero, Node::new("Score", NodeKind::Label(Label::default()))).unwrap();
    for _ in 0..10 {
        inst.trigger(EventKind::Process, StringName::EMPTY, &[Variant::Number(0.1)], &mut tree, &mut host).unwrap();
    }
    let p = tree.get(hero).unwrap().transform.position;
    assert!((p.x - 100.0).abs() < 1e-3, "{p}");
    assert_eq!(tree.get(label).unwrap().get("text").unwrap().to_string(), "x = 100");
}

#[test]
fn delay_flipflop_doonce() {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.custom", (0.0, 0.0), &[("name", "tick".into())]);
    let seq = bp.add("flow.sequence", (100.0, 0.0), &[]);
    let once = bp.add("flow.do_once", (200.0, 0.0), &[]);
    let p_once = bp.add("debug.print", (300.0, 0.0), &[("value", "once".into())]);
    let ff = bp.add("flow.flip_flop", (200.0, 100.0), &[]);
    let pa = bp.add("debug.print", (300.0, 100.0), &[("value", "A".into())]);
    let pb = bp.add("debug.print", (300.0, 200.0), &[("value", "B".into())]);
    let dl = bp.add("flow.delay", (200.0, 300.0), &[("seconds", 0.5.into())]);
    let pd = bp.add("debug.print", (300.0, 300.0), &[("value", "late".into())]);
    link(&mut bp, ev, "then", seq, "exec");
    link(&mut bp, seq, "then 0", once, "exec");
    link(&mut bp, once, "then", p_once, "exec");
    link(&mut bp, seq, "then 1", ff, "exec");
    link(&mut bp, ff, "a", pa, "exec");
    link(&mut bp, ff, "b", pb, "exec");
    link(&mut bp, seq, "then 2", dl, "exec");
    link(&mut bp, dl, "completed", pd, "exec");
    let (mut tree, mut inst, mut host) = setup(&bp);
    let tick = StringName::new("tick");
    for _ in 0..3 {
        inst.trigger(EventKind::Custom, tick, &[], &mut tree, &mut host).unwrap();
    }
    // Other event names do not match the filter.
    inst.trigger(EventKind::Custom, StringName::new("other"), &[], &mut tree, &mut host).unwrap();
    assert_eq!(host.log, vec!["[Hero] once", "[Hero] A", "[Hero] B", "[Hero] A"]);
    assert_eq!(inst.pending_delays(), 3);
    inst.update_delays(0.3, &mut tree, &mut host).unwrap();
    assert_eq!(host.log.len(), 4);
    inst.update_delays(0.3, &mut tree, &mut host).unwrap();
    assert_eq!(host.log.iter().filter(|l| l.ends_with("late")).count(), 3);
}

#[test]
fn infinite_while_is_caught() {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.ready", (0.0, 0.0), &[]);
    let wl = bp.add("flow.while", (100.0, 0.0), &[("condition", true.into())]);
    link(&mut bp, ev, "then", wl, "exec");
    let (mut tree, mut inst, mut host) = setup(&bp);
    let err = inst.trigger(EventKind::Ready, StringName::EMPTY, &[], &mut tree, &mut host).unwrap_err();
    assert!(err.message.contains("infinite"));
}

#[test]
fn foreach_over_group_destroys_enemies() {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.ready", (0.0, 0.0), &[]);
    let grp = bp.add("node.group", (0.0, 100.0), &[("group", "enemies".into())]);
    let fe = bp.add("flow.foreach", (100.0, 0.0), &[]);
    let kill = bp.add("node.destroy", (200.0, 0.0), &[]);
    link(&mut bp, ev, "then", fe, "exec");
    link(&mut bp, grp, "nodes", fe, "array");
    link(&mut bp, fe, "body", kill, "exec");
    link(&mut bp, fe, "element", kill, "node");
    let (mut tree, mut inst, mut host) = setup(&bp);
    for i in 0..5 {
        let n = Node::new(format!("E{i}"), NodeKind::Node2D(Node2D::default())).with_group("enemies");
        tree.add_child(tree.root(), n).unwrap();
    }
    assert_eq!(tree.len(), 7);
    fire(&mut inst, &mut tree, &mut host, EventKind::Ready);
    tree.flush_queued();
    assert_eq!(tree.len(), 2);
}

#[test]
fn pure_nodes_reevaluate_per_iteration() {
    // For 0..3: Print(Random Int 0..0 + index) - the Add must see each index.
    let mut bp = Blueprint::new();
    let ev = bp.add("event.ready", (0.0, 0.0), &[]);
    let lp = bp.add("flow.for", (100.0, 0.0), &[("first", 0.0.into()), ("last", 3.0.into())]);
    let add = bp.add("math.add", (200.0, 100.0), &[("b", 10.0.into())]);
    let pr = bp.add("debug.print", (300.0, 0.0), &[]);
    link(&mut bp, ev, "then", lp, "exec");
    link(&mut bp, lp, "body", pr, "exec");
    link(&mut bp, lp, "index", add, "a");
    link(&mut bp, add, "result", pr, "value");
    let (mut tree, mut inst, mut host) = setup(&bp);
    fire(&mut inst, &mut tree, &mut host, EventKind::Ready);
    assert_eq!(host.log, vec!["[Hero] 10", "[Hero] 11", "[Hero] 12", "[Hero] 13"]);
}

#[test]
fn json_roundtrip_compiles_identically() {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.ready", (0.0, 0.0), &[]);
    let pr = bp.add("debug.print", (100.0, 0.0), &[("value", "hi".into())]);
    link(&mut bp, ev, "then", pr, "exec");
    let back = Blueprint::from_json(&bp.to_json()).unwrap();
    let a = compile(&bp).unwrap();
    let b = compile(&back).unwrap();
    assert_eq!(a.code, b.code);
    assert!(a.disassemble().contains("Print"));
}
