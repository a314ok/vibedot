//! End-to-end runtime tests: a tiny game assembled in memory.

use std::sync::Arc;

use vd_blueprint::{Blueprint, PinRef};
use vd_core::{Variant, vec2};
use vd_scene::*;

use crate::assets::MemAssets;
use crate::game::Game;
use crate::project::ProjectSettings;
use crate::script::{NodeScript, ScriptCtx, ScriptRegistry};

fn link(bp: &mut Blueprint, a: vd_blueprint::BpNodeId, ap: &str, b: vd_blueprint::BpNodeId, bpin: &str) {
    bp.connect(PinRef::new(a, ap), PinRef::new(b, bpin)).unwrap_or_else(|e| panic!("{ap}->{bpin}: {e}"));
}

/// Player: On Process → Set Velocity(Input Vector * speed).
fn player_bp() -> Blueprint {
    let mut bp = Blueprint::new();
    bp.add_variable("speed", vd_core::VType::Number, Variant::Number(200.0));
    let ev = bp.add("event.process", (0.0, 0.0), &[]);
    let iv = bp.add("input.vector", (0.0, 100.0), &[]);
    let sp = bp.add("var.get", (0.0, 200.0), &[("var", "speed".into())]);
    let mul = bp.add("math.mul", (100.0, 100.0), &[]);
    let set = bp.add("physics.set_velocity", (200.0, 0.0), &[]);
    link(&mut bp, ev, "then", set, "exec");
    link(&mut bp, iv, "vector", mul, "a");
    link(&mut bp, sp, "value", mul, "b");
    link(&mut bp, mul, "result", set, "velocity");
    bp
}

/// Spawner (on the scene root): On Timer Timeout → Spawn "coin.vscn" at (300, 0).
fn spawner_bp() -> Blueprint {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.timeout", (0.0, 0.0), &[("timer", "SpawnTimer".into())]);
    let spawn = bp.add("node.spawn", (100.0, 0.0), &[("scene", "coin.vscn".into()), ("position", Variant::Vec2(vec2(300.0, 0.0)))]);
    let pr = bp.add("debug.print", (200.0, 0.0), &[("value", "spawned".into())]);
    link(&mut bp, ev, "then", spawn, "exec");
    link(&mut bp, spawn, "then", pr, "exec");
    bp
}

/// Coin: method `_on_body_entered` (via persisted signal connection) →
/// Emit Event "collected" + Destroy self.
fn coin_bp() -> Blueprint {
    let mut bp = Blueprint::new();
    let ev = bp.add("event.custom", (0.0, 0.0), &[("name", "_on_body_entered".into())]);
    let emit = bp.add("game.emit", (100.0, 0.0), &[("name", "collected".into())]);
    let kill = bp.add("node.destroy", (200.0, 0.0), &[]);
    link(&mut bp, ev, "then", emit, "exec");
    link(&mut bp, emit, "then", kill, "exec");
    bp
}

/// Score keeper: On Event "collected" → score += 1 → Set Text("Score: {0}").
fn score_bp() -> Blueprint {
    let mut bp = Blueprint::new();
    bp.add_variable("score", vd_core::VType::Number, Variant::Number(0.0));
    let ev = bp.add("event.custom", (0.0, 0.0), &[("name", "collected".into())]);
    let add = bp.add("var.add", (100.0, 0.0), &[("var", "score".into())]);
    let fmt = bp.add("str.format", (100.0, 100.0), &[("template", "Score: {0}".into())]);
    let set = bp.add("node.set_text", (200.0, 0.0), &[]);
    link(&mut bp, ev, "then", add, "exec");
    link(&mut bp, add, "then", set, "exec");
    link(&mut bp, add, "value", fmt, "0");
    link(&mut bp, fmt, "result", set, "text");
    bp
}

fn scene_json(tree: &SceneTree) -> String {
    SceneFile::from_tree(tree).to_json()
}

fn build_assets() -> MemAssets {
    let mut assets = MemAssets::default();
    assets.insert("player.vbp", player_bp().to_json());
    assets.insert("spawner.vbp", spawner_bp().to_json());
    assets.insert("coin.vbp", coin_bp().to_json());
    assets.insert("score.vbp", score_bp().to_json());

    // coin.vscn: Area2D with a persisted body_entered → _on_body_entered connection.
    let mut t = SceneTree::new();
    let root = t.root();
    let mut coin = Node::new("Coin", NodeKind::Body2D(Body2D { body_type: BodyType::Area, shape: CollisionShape::Circle, radius: 16.0, ..Default::default() }));
    coin.script = Some("coin.vbp".into());
    coin.add_to_group("coins");
    let coin = t.add_new_child(root, coin).unwrap();
    t.set_current_scene(Some(coin));
    t.connect(coin, "body_entered", coin, "_on_body_entered", connect_flags::PERSIST).unwrap();
    assets.insert("coin.vscn", scene_json(&t));

    // main.vscn
    let mut t = SceneTree::new();
    let root = t.root();
    let mut main = Node::new("Main", NodeKind::Node2D(Node2D::default()));
    main.script = Some("spawner.vbp".into());
    let main = t.add_new_child(root, main).unwrap();
    t.set_current_scene(Some(main));
    let mut player = Node::new("Player", NodeKind::Body2D(Body2D { body_type: BodyType::Character, size: vec2(32.0, 32.0), ..Default::default() }));
    player.script = Some("player.vbp".into());
    t.add_new_child(main, player).unwrap();
    t.add_new_child(main, Node::new("SpawnTimer", NodeKind::Timer(Timer { wait_time: 0.5, one_shot: true, autostart: true, ..Default::default() }))).unwrap();
    let mut score = Node::new("Score", NodeKind::Label(Label { text: "Score: 0".into(), ..Default::default() }));
    score.script = Some("score.vbp".into());
    score.unique_name_in_owner = true;
    t.add_new_child(main, score).unwrap();
    assets.insert("main.vscn", scene_json(&t));
    assets
}

fn project() -> ProjectSettings {
    ProjectSettings { main_scene: "main.vscn".into(), ..Default::default() }
}

#[test]
fn mini_game_plays_through() {
    let mut game = Game::new(project(), Arc::new(build_assets()), Arc::new(ScriptRegistry::default()));
    assert!(game.start(), "{:?}", game.host.log);
    let main = game.tree.current_scene().unwrap();
    let player = game.tree.get_node(main, "Player").unwrap();
    let score = game.tree.get_node(main, "%Score").unwrap();
    assert!(game.tree.get(player).unwrap().is_processing(), "On Process enables processing");

    // Hold "D" (ui_right): the player walks right toward the coin spawn point.
    game.host.input.key_down("D");
    for _ in 0..120 {
        game.update(1.0 / 60.0);
        game.host.input.begin_frame();
    }
    let errors: Vec<_> = game.host.log.iter().filter(|l| l.level == crate::LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(game.host.log.iter().any(|l| l.text.contains("spawned")), "timer fired and spawned a coin");
    let px = game.tree.global_position(player).x;
    assert!(px > 300.0, "player moved right: {px}");
    // The coin was touched: signal → blueprint → Emit Event → score label.
    assert!(game.tree.get_nodes_in_group("coins").is_empty(), "coin destroyed");
    let text = game.tree.get(score).unwrap().get("text").unwrap().to_string();
    assert_eq!(text, "Score: 1");
}

#[test]
fn pause_stops_pausable_nodes() {
    let mut game = Game::new(project(), Arc::new(build_assets()), Arc::new(ScriptRegistry::default()));
    game.start();
    let main = game.tree.current_scene().unwrap();
    let player = game.tree.get_node(main, "Player").unwrap();
    game.tree.set_pause(true);
    game.host.input.key_down("D");
    for _ in 0..30 {
        game.update(1.0 / 60.0);
        game.host.input.begin_frame();
    }
    assert_eq!(game.tree.global_position(player).x, 0.0);
    game.tree.set_pause(false);
    for _ in 0..30 {
        game.update(1.0 / 60.0);
    }
    assert!(game.tree.global_position(player).x > 0.0);
}

struct Spinner {
    ticks: u32,
}

impl NodeScript for Spinner {
    fn ready(&mut self, cx: &mut ScriptCtx<'_>) {
        cx.print("spinner ready");
    }
    fn process(&mut self, cx: &mut ScriptCtx<'_>, delta: f32) {
        self.ticks += 1;
        if let Some(n) = cx.node_mut() {
            n.transform.rotation += 1.0 * delta;
        }
        if self.ticks == 10 {
            cx.emit_signal("spun", &[Variant::from(10.0)]);
        }
    }
    fn call(&mut self, cx: &mut ScriptCtx<'_>, method: &str, args: &[Variant]) -> bool {
        cx.print(format!("{method}({})", args.first().map(ToString::to_string).unwrap_or_default()));
        true
    }
}

#[test]
fn native_rust_scripts() {
    let mut registry = ScriptRegistry::default();
    registry.register("Spinner", || Box::new(Spinner { ticks: 0 }));
    let mut assets = MemAssets::default();
    let mut t = SceneTree::new();
    let root = t.root();
    let mut a = Node::new("A", NodeKind::Node2D(Node2D::default()));
    a.script = Some("rust:Spinner".into());
    let a = t.add_new_child(root, a).unwrap();
    t.set_current_scene(Some(a));
    let mut b = Node::new("B", NodeKind::Node2D(Node2D::default()));
    b.script = Some("rust:Spinner".into());
    let b = t.add_new_child(a, b).unwrap();
    t.connect(a, "spun", b, "_on_spun", connect_flags::PERSIST).unwrap();
    assets.insert("s.vscn", SceneFile::from_tree(&t).to_json());
    let mut game = Game::new(ProjectSettings { main_scene: "s.vscn".into(), ..Default::default() }, Arc::new(assets), Arc::new(registry));
    assert!(game.start());
    for _ in 0..12 {
        game.update(0.1);
    }
    let a = game.tree.current_scene().unwrap();
    assert!((game.tree.get(a).unwrap().transform.rotation - 1.2).abs() < 1e-3);
    let logs: Vec<&str> = game.host.log.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(logs.iter().filter(|l| **l == "spinner ready").count(), 2);
    assert!(logs.contains(&"_on_spun(10)"), "{logs:?}");
}

#[test]
fn draw_list_contains_scene() {
    let mut game = Game::new(project(), Arc::new(build_assets()), Arc::new(ScriptRegistry::default()));
    game.start();
    game.update(0.016);
    let mut list = vd_render::DrawList::default();
    game.draw(&mut list);
    assert_eq!(list.world.len(), 1, "the score label");
    assert_eq!(list.view.center, vec2(640.0, 360.0), "no camera: origin at top-left like Godot");
}
