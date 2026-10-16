//! Project templates. Everything is built through the public scene and
//! blueprint APIs, so templates double as executable documentation.

use vd_blueprint::{BpNodeId, Blueprint, PinRef};
use vd_core::{Color, Transform2D, VType, Variant, Vec2, vec2};
use vd_scene::*;

use crate::project::ProjectSettings;

/// A generated file: project-relative path and contents.
pub type File = (String, Vec<u8>);

/// Tiny builder for readable graph construction.
struct G {
    bp: Blueprint,
}

impl G {
    fn new() -> Self {
        Self { bp: Blueprint::new() }
    }
    fn var(&mut self, name: &str, ty: VType, v: Variant) -> &mut Self {
        self.bp.add_variable(name, ty, v);
        self
    }
    fn n(&mut self, op: &str, x: f32, y: f32, vals: &[(&str, Variant)]) -> BpNodeId {
        assert!(vd_blueprint::defs::find(op).is_some(), "unknown op {op}");
        self.bp.add(op, (x, y), vals)
    }
    fn l(&mut self, a: BpNodeId, ap: &str, b: BpNodeId, bpin: &str) {
        self.bp.connect(PinRef::new(a, ap), PinRef::new(b, bpin)).unwrap_or_else(|e| panic!("template link {ap}->{bpin}: {e}"));
    }
    /// Links a chain of exec nodes: `then` → `exec`.
    fn chain(&mut self, nodes: &[(BpNodeId, &str)]) {
        for w in nodes.windows(2) {
            self.l(w[0].0, w[0].1, w[1].0, "exec");
        }
    }
    fn comment(&mut self, x: f32, y: f32, w: f32, h: f32, text: &str, color: Color) {
        self.bp.comments.push(vd_blueprint::graph::Comment { pos: vec2(x, y), size: vec2(w, h), text: text.into(), color });
    }
    fn json(&self) -> Vec<u8> {
        vd_blueprint::compile(&self.bp).unwrap_or_else(|e| panic!("template blueprint does not compile: {e:?}"));
        self.bp.to_json().into_bytes()
    }
}

fn s(v: &str) -> Variant {
    Variant::str(v)
}
fn n(v: f64) -> Variant {
    Variant::Number(v)
}
fn hex(h: &str) -> Color {
    Color::from_hex(h).expect("valid hex")
}

fn node(name: &str, kind: NodeKind) -> Node {
    Node::new(name, kind)
}

fn at(mut n: Node, x: f32, y: f32) -> Node {
    n.transform = Transform2D::from_position(vec2(x, y));
    n
}

fn scene_file(tree: &SceneTree) -> Vec<u8> {
    SceneFile::from_tree(tree).to_json().into_bytes()
}

/// New tree with `root` as the current scene.
fn scene(root: Node) -> (SceneTree, NodeId) {
    let mut t = SceneTree::new();
    let r = t.root();
    let id = t.add_new_child(r, root).expect("root exists");
    t.set_current_scene(Some(id));
    (t, id)
}

fn shape(shape: ShapeType, size: Vec2, color: Color, emission: f32) -> NodeKind {
    NodeKind::Shape2D(Shape2D { shape, size, color, emission, ..Default::default() })
}

fn label(text: &str, size: f32, color: Color, align: TextAlign, screen: bool) -> NodeKind {
    NodeKind::Label(Label { text: text.into(), font_size: size, color, align, screen_space: screen, ..Default::default() })
}

fn light(color: Color, energy: f32, radius: f32) -> NodeKind {
    NodeKind::PointLight2D(PointLight2D { color, energy, radius, ..Default::default() })
}

/// Minimal empty project: one scene with a camera, an environment and a label.
pub fn empty_project(name: &str) -> Vec<File> {
    let project = ProjectSettings { name: name.into(), main_scene: "scenes/main.vscn".into(), ..Default::default() };
    let (mut t, main) = scene(node("Main", NodeKind::Node2D(Node2D::default())));
    t.add_new_child(main, node("WorldEnvironment", NodeKind::WorldEnvironment(WorldEnvironment::default()))).unwrap();
    t.add_new_child(main, node("Camera2D", NodeKind::Camera2D(Camera2D::default()))).unwrap();
    let mut hello = node("Hello", label(&format!("Hello, {name}!"), 48.0, Color::WHITE, TextAlign::Center, false));
    hello.script = Some("blueprints/hello.vbp".into());
    t.add_new_child(main, hello).unwrap();

    let mut g = G::new();
    let ev = g.n("event.process", 0.0, 0.0, &[]);
    let wave = g.n("math.wave", 0.0, 140.0, &[("frequency", n(0.5)), ("amplitude", n(8.0))]);
    let rot = g.n("node.set_rotation", 280.0, 0.0, &[]);
    g.chain(&[(ev, "then"), (rot, "then")]);
    g.l(wave, "result", rot, "degrees");
    vec![
        ("project.vibedot".into(), project.to_json().into_bytes()),
        ("scenes/main.vscn".into(), scene_file(&t)),
        ("blueprints/hello.vbp".into(), g.json()),
    ]
}

/// "Neon Drift": a complete top-down arcade game built only with blueprints.
pub fn neon_drift() -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    let mut project = ProjectSettings {
        name: "Neon Drift".into(),
        main_scene: "scenes/main.vscn".into(),
        ..Default::default()
    };
    project.physics.gravity = Vec2::ZERO;
    files.push(("project.vibedot".into(), project.to_json().into_bytes()));

    let cyan = hex("#22d3ee");
    let violet = hex("#8b5cf6");
    let magenta = hex("#f43f5e");
    let gold = hex("#fbbf24");

    // ------------------------------------------------------------ main scene
    let mut main_node = node("Main", NodeKind::Node2D(Node2D::default()));
    main_node.script = Some("blueprints/main.vbp".into());
    let (mut t, main) = scene(main_node);
    t.add_new_child(
        main,
        node(
            "WorldEnvironment",
            NodeKind::WorldEnvironment(WorldEnvironment {
                clear_color: hex("#05060f"),
                ambient: 0.32,
                ambient_color: hex("#9aa6ff"),
                bloom_intensity: 0.9,
                bloom_threshold: 0.75,
                vignette: 0.55,
                chromatic_aberration: 1.4,
                film_grain: 0.035,
                scanlines: 0.12,
                saturation: 1.15,
                contrast: 1.08,
                ..Default::default()
            }),
        ),
    )
    .unwrap();

    // Backdrop: soft nebulae and a starfield.
    let bg = t.add_new_child(main, node("Backdrop", NodeKind::Node2D(Node2D::default()))).unwrap();
    t.get_mut(bg).unwrap().z_index = -50;
    for (i, (x, y, r, c)) in [(-500.0, -300.0, 900.0, violet), (600.0, 350.0, 1100.0, cyan), (-200.0, 500.0, 700.0, magenta)]
        .into_iter()
        .enumerate()
    {
        let mut neb = Shape2D { shape: ShapeType::Circle, size: vec2(r, r), color: c.with_alpha(0.12), ..Default::default() };
        neb.softness = r * 0.45;
        neb.blend = BlendMode::Additive;
        t.add_new_child(bg, at(node(&format!("Nebula{i}"), NodeKind::Shape2D(neb)), x, y)).unwrap();
    }
    let stars = Particles2D {
        rate: 40.0,
        burst: 400,
        lifetime: 12.0,
        lifetime_random: 0.6,
        direction: 0.0,
        spread: 3.14,
        speed_min: 2.0,
        speed_max: 8.0,
        gravity: Vec2::ZERO,
        damping: 0.0,
        emit_radius: 1400.0,
        size_start: 3.0,
        size_end: 3.0,
        color_start: Color::rgba(0.8, 0.85, 1.0, 0.9),
        color_end: Color::rgba(0.8, 0.85, 1.0, 0.0),
        emission: 1.2,
        max_particles: 1200,
        ..Default::default()
    };
    t.add_new_child(bg, node("Stars", NodeKind::Particles2D(stars))).unwrap();

    // Arena walls (static bodies with glowing edges).
    let walls = t.add_new_child(main, node("Arena", NodeKind::Node2D(Node2D::default()))).unwrap();
    let (aw, ah) = (2000.0f32, 1400.0f32);
    for (name, x, y, w, h) in [
        ("Top", 0.0, -ah / 2.0, aw, 24.0),
        ("Bottom", 0.0, ah / 2.0, aw, 24.0),
        ("Left", -aw / 2.0, 0.0, 24.0, ah),
        ("Right", aw / 2.0, 0.0, 24.0, ah),
    ] {
        let mut wall = node(
            name,
            NodeKind::Body2D(Body2D { body_type: BodyType::Static, shape: CollisionShape::Rect, size: vec2(w, h), ..Default::default() }),
        );
        wall.add_to_group("walls");
        let wall = t.add_new_child(walls, at(wall, x, y)).unwrap();
        let mut edge = Shape2D { shape: ShapeType::RoundedRect, size: vec2(w, h * 0.25), corner_radius: 4.0, color: violet, emission: 2.5, ..Default::default() };
        if w < h {
            edge.size = vec2(w * 0.25, h);
        }
        t.add_new_child(wall, node("Glow", NodeKind::Shape2D(edge))).unwrap();
    }

    // Player ship.
    let mut player = node(
        "Player",
        NodeKind::Body2D(Body2D { body_type: BodyType::Character, shape: CollisionShape::Circle, radius: 18.0, ..Default::default() }),
    );
    player.add_to_group("player");
    player.script = Some("blueprints/player.vbp".into());
    player.unique_name_in_owner = true;
    let player = t.add_new_child(main, player).unwrap();
    t.add_new_child(player, node("Hull", shape(ShapeType::Triangle, vec2(58.0, 42.0), cyan, 1.3))).unwrap();
    t.add_new_child(player, at(node("Core", shape(ShapeType::Circle, vec2(10.0, 10.0), Color::WHITE, 4.0)), -4.0, 0.0)).unwrap();
    t.add_new_child(player, node("Light", light(cyan, 1.8, 320.0))).unwrap();
    let trail = Particles2D {
        rate: 110.0,
        lifetime: 0.55,
        direction: std::f32::consts::PI,
        spread: 0.35,
        speed_min: 60.0,
        speed_max: 160.0,
        gravity: Vec2::ZERO,
        damping: 2.0,
        size_start: 10.0,
        size_end: 0.0,
        color_start: cyan,
        color_end: violet.with_alpha(0.0),
        emission: 3.0,
        ..Default::default()
    };
    t.add_new_child(player, at(node("Trail", NodeKind::Particles2D(trail)), -18.0, 0.0)).unwrap();
    t.add_new_child(player, node("Camera2D", NodeKind::Camera2D(Camera2D { smoothing: 6.0, zoom: 1.0, ..Default::default() }))).unwrap();

    // Timers.
    t.add_new_child(main, node("SpawnTimer", NodeKind::Timer(Timer { wait_time: 1.1, ..Default::default() }))).unwrap();
    t.add_new_child(main, node("OrbTimer", NodeKind::Timer(Timer { wait_time: 1.8, ..Default::default() }))).unwrap();

    // HUD.
    let hud = t.add_new_child(main, node("HUD", NodeKind::Node(PlainNode::default()))).unwrap();
    let mut score = at(node("Score", label("SCORE  0", 26.0, Color::WHITE, TextAlign::Left, true)), 28.0, 34.0);
    score.unique_name_in_owner = true;
    if let NodeKind::Label(l) = &mut score.kind {
        l.monospace = true;
    }
    t.add_new_child(hud, score).unwrap();
    t.add_new_child(hud, at(node("Hint", label("WASD / arrows: fly    J or click: shoot    Esc: quit", 16.0, hex("#94a3b8"), TextAlign::Left, true)), 28.0, 70.0))
        .unwrap();
    let mut title = at(node("Title", label("NEON DRIFT", 64.0, cyan, TextAlign::Center, false)), 0.0, -560.0);
    if let NodeKind::Label(l) = &mut title.kind {
        l.emission = 2.0;
    }
    t.add_new_child(main, title).unwrap();
    let mut over = node("GameOver", label("GAME OVER", 96.0, magenta, TextAlign::Center, false));
    over.visible = false;
    over.unique_name_in_owner = true;
    over.z_index = 100;
    if let NodeKind::Label(l) = &mut over.kind {
        l.emission = 3.0;
    }
    t.add_new_child(player, over).unwrap();
    files.push(("scenes/main.vscn".into(), scene_file(&t)));

    // ------------------------------------------------------------ enemy
    let mut enemy = node(
        "Enemy",
        NodeKind::Body2D(Body2D { body_type: BodyType::Character, shape: CollisionShape::Circle, radius: 17.0, ..Default::default() }),
    );
    enemy.add_to_group("enemies");
    enemy.script = Some("blueprints/enemy.vbp".into());
    let (mut t, e) = scene(enemy);
    let mut ring = Shape2D { shape: ShapeType::Ring, size: vec2(38.0, 38.0), thickness: 5.0, color: magenta, emission: 3.0, ..Default::default() };
    ring.softness = 1.0;
    t.add_new_child(e, node("Ring", NodeKind::Shape2D(ring))).unwrap();
    t.add_new_child(e, node("Core", shape(ShapeType::Triangle, vec2(16.0, 16.0), gold, 2.5))).unwrap();
    t.add_new_child(e, node("Light", light(magenta, 1.2, 180.0))).unwrap();
    files.push(("scenes/enemy.vscn".into(), scene_file(&t)));

    // ------------------------------------------------------------ bullet
    let mut bullet = node(
        "Bullet",
        NodeKind::Body2D(Body2D { body_type: BodyType::Area, shape: CollisionShape::Circle, radius: 7.0, layer: 2, mask: 1, ..Default::default() }),
    );
    bullet.script = Some("blueprints/bullet.vbp".into());
    let (mut t, b) = scene(bullet);
    let mut core = Shape2D { shape: ShapeType::Circle, size: vec2(12.0, 12.0), color: Color::rgb(0.7, 1.0, 1.0), emission: 5.0, ..Default::default() };
    core.softness = 2.0;
    t.add_new_child(b, node("Core", NodeKind::Shape2D(core))).unwrap();
    t.add_new_child(b, node("Light", light(cyan, 0.8, 90.0))).unwrap();
    files.push(("scenes/bullet.vscn".into(), scene_file(&t)));

    // ------------------------------------------------------------ orb
    let mut orb = node(
        "Orb",
        NodeKind::Body2D(Body2D { body_type: BodyType::Area, shape: CollisionShape::Circle, radius: 16.0, layer: 2, mask: 1, ..Default::default() }),
    );
    orb.script = Some("blueprints/orb.vbp".into());
    orb.add_to_group("orbs");
    let (mut t, o) = scene(orb);
    let mut glow = Shape2D { shape: ShapeType::Circle, size: vec2(26.0, 26.0), color: gold, emission: 2.5, ..Default::default() };
    glow.softness = 3.0;
    t.add_new_child(o, node("Glow", NodeKind::Shape2D(glow))).unwrap();
    let mut halo = Shape2D { shape: ShapeType::Ring, size: vec2(40.0, 40.0), thickness: 2.0, color: gold.with_alpha(0.7), emission: 2.0, ..Default::default() };
    halo.blend = BlendMode::Additive;
    t.add_new_child(o, node("Halo", NodeKind::Shape2D(halo))).unwrap();
    t.add_new_child(o, node("Light", light(gold, 1.0, 140.0))).unwrap();
    files.push(("scenes/orb.vscn".into(), scene_file(&t)));

    // ------------------------------------------------------------ explosion
    let mut boom = node("Explosion", NodeKind::Node2D(Node2D::default()));
    boom.script = Some("blueprints/explosion.vbp".into());
    let (mut t, x) = scene(boom);
    let sparks = Particles2D {
        one_shot: true,
        rate: 0.0,
        burst: 70,
        lifetime: 0.75,
        spread: 3.14,
        speed_min: 80.0,
        speed_max: 460.0,
        gravity: Vec2::ZERO,
        damping: 3.0,
        size_start: 9.0,
        size_end: 0.0,
        color_start: hex("#fde68a"),
        color_end: magenta.with_alpha(0.0),
        emission: 4.0,
        ..Default::default()
    };
    t.add_new_child(x, node("Sparks", NodeKind::Particles2D(sparks))).unwrap();
    t.add_new_child(x, node("Flash", light(hex("#fb923c"), 3.0, 260.0))).unwrap();
    files.push(("scenes/explosion.vscn".into(), scene_file(&t)));

    files.push(("blueprints/main.vbp".into(), main_bp()));
    files.push(("blueprints/player.vbp".into(), player_bp()));
    files.push(("blueprints/enemy.vbp".into(), enemy_bp()));
    files.push(("blueprints/bullet.vbp".into(), bullet_bp()));
    files.push(("blueprints/orb.vbp".into(), orb_bp()));
    files.push(("blueprints/explosion.vbp".into(), explosion_bp()));
    files
}

fn main_bp() -> Vec<u8> {
    let mut g = G::new();
    g.var("score", VType::Number, n(0.0));
    let comment_color = Color::rgba(0.55, 0.36, 0.96, 0.25);

    // Spawn enemies on a ring around the player.
    g.comment(-40.0, -60.0, 1000.0, 300.0, "Spawn enemies around the player", comment_color);
    let ev = g.n("event.timeout", 0.0, 0.0, &[("timer", s("SpawnTimer"))]);
    let player = g.n("node.first_in_group", 0.0, 120.0, &[("group", s("player"))]);
    let ppos = g.n("node.get_position", 220.0, 120.0, &[]);
    let ang = g.n("math.random", 220.0, 200.0, &[("min", n(0.0)), ("max", n(360.0))]);
    let off = g.n("vec.from_angle", 440.0, 200.0, &[("length", n(620.0))]);
    let sum = g.n("math.add", 640.0, 140.0, &[]);
    let inside = g.n(
        "vec.clamp",
        640.0,
        240.0,
        &[("min", Variant::Vec2(vec2(-940.0, -640.0))), ("max", Variant::Vec2(vec2(940.0, 640.0)))],
    );
    let spawn = g.n("node.spawn", 860.0, 0.0, &[("scene", s("scenes/enemy.vscn"))]);
    g.l(ev, "then", spawn, "exec");
    g.l(player, "node", ppos, "node");
    g.l(ang, "result", off, "degrees");
    g.l(ppos, "position", sum, "a");
    g.l(off, "vector", sum, "b");
    g.l(sum, "result", inside, "vector");
    g.l(inside, "result", spawn, "position");

    // Spawn orbs inside the arena.
    g.comment(-40.0, 300.0, 1000.0, 200.0, "Spawn energy orbs", comment_color);
    let ev2 = g.n("event.timeout", 0.0, 360.0, &[("timer", s("OrbTimer"))]);
    let rnd = g.n("vec.random_in_circle", 220.0, 440.0, &[("radius", n(620.0))]);
    let spawn2 = g.n("node.spawn", 820.0, 360.0, &[("scene", s("scenes/orb.vscn"))]);
    g.l(ev2, "then", spawn2, "exec");
    g.l(rnd, "point", spawn2, "position");

    // Score handling.
    g.comment(-40.0, 560.0, 1300.0, 520.0, "Score: orbs +10, enemies +25", comment_color);
    for (i, (event, amount)) in [("orb", 10.0), ("enemy_killed", 25.0)].into_iter().enumerate() {
        let y = 620.0 + i as f32 * 240.0;
        let ev = g.n("event.custom", 0.0, y, &[("name", s(event))]);
        let add = g.n("var.add", 260.0, y, &[("var", s("score")), ("amount", n(amount))]);
        let label = g.n("node.get_node", 260.0, y + 110.0, &[("path", s("%Score"))]);
        let fmt = g.n("str.format", 520.0, y + 110.0, &[("template", s("SCORE  {0}"))]);
        let set = g.n("node.set_text", 780.0, y, &[]);
        g.chain(&[(ev, "then"), (add, "then"), (set, "then")]);
        g.l(add, "value", fmt, "0");
        g.l(fmt, "result", set, "text");
        g.l(label, "node", set, "node");
        if event == "enemy_killed" {
            let boom = g.n("node.spawn", 1040.0, y, &[("scene", s("scenes/explosion.vscn"))]);
            let shake = g.n("game.shake", 1300.0, y, &[("amount", n(0.35))]);
            g.chain(&[(set, "then"), (boom, "then"), (shake, "then")]);
            g.l(ev, "payload", boom, "position");
        } else {
            let flash = g.n("game.flash", 1040.0, y, &[("color", Variant::Color(Color::rgba(1.0, 0.85, 0.3, 0.18))), ("duration", n(0.12))]);
            g.l(set, "then", flash, "exec");
        }
    }

    // Game over: slow motion, flash, restart.
    g.comment(-40.0, 1120.0, 1600.0, 260.0, "Game over → slow motion → restart", comment_color);
    let ev = g.n("event.custom", 0.0, 1180.0, &[("name", s("game_over"))]);
    let once = g.n("flow.do_once", 240.0, 1180.0, &[]);
    let over = g.n("node.get_node", 240.0, 1300.0, &[("path", s("%GameOver"))]);
    let show = g.n("node.set_visible", 460.0, 1180.0, &[]);
    let slow = g.n("game.time_scale", 680.0, 1180.0, &[("scale", n(0.25))]);
    let flash = g.n("game.flash", 900.0, 1180.0, &[("color", Variant::Color(Color::rgba(1.0, 0.2, 0.3, 0.55))), ("duration", n(0.5))]);
    let shake = g.n("game.shake", 1120.0, 1180.0, &[("amount", n(1.0))]);
    let delay = g.n("flow.delay", 1340.0, 1180.0, &[("seconds", n(0.6))]);
    let normal = g.n("game.time_scale", 1560.0, 1180.0, &[("scale", n(1.0))]);
    let restart = g.n("game.reload", 1780.0, 1180.0, &[]);
    g.chain(&[(ev, "then"), (once, "then"), (show, "then"), (slow, "then"), (flash, "then"), (shake, "then"), (delay, "completed"), (normal, "then"), (restart, "then")]);
    g.l(over, "node", show, "node");

    // Quit on Escape; start the timers on ready.
    let esc = g.n("event.key_pressed", 0.0, 1460.0, &[("key", s("Escape"))]);
    let quit = g.n("game.quit", 240.0, 1460.0, &[]);
    g.l(esc, "then", quit, "exec");
    let ready = g.n("event.ready", 0.0, 1580.0, &[]);
    let t1 = g.n("node.get_node", 0.0, 1680.0, &[("path", s("SpawnTimer"))]);
    let t2 = g.n("node.get_node", 0.0, 1780.0, &[("path", s("OrbTimer"))]);
    let s1 = g.n("timer.start", 240.0, 1580.0, &[]);
    let s2 = g.n("timer.start", 460.0, 1580.0, &[]);
    let hello = g.n("debug.print", 680.0, 1580.0, &[("value", s("Neon Drift started - good luck!"))]);
    g.chain(&[(ready, "then"), (s1, "then"), (s2, "then"), (hello, "then")]);
    g.l(t1, "node", s1, "timer");
    g.l(t2, "node", s2, "timer");
    g.json()
}

fn player_bp() -> Vec<u8> {
    let mut g = G::new();
    g.var("speed", VType::Number, n(430.0)).var("bullet_speed", VType::Number, n(950.0));
    // Smooth steering: velocity = lerp(velocity, input * speed, 0.18)
    let ev = g.n("event.process", 0.0, 0.0, &[]);
    let input = g.n("input.vector", 0.0, 120.0, &[]);
    let speed = g.n("var.get", 0.0, 230.0, &[("var", s("speed"))]);
    let target = g.n("math.mul", 230.0, 160.0, &[]);
    let vel = g.n("physics.get_velocity", 230.0, 280.0, &[]);
    let lerp = g.n("math.lerp", 460.0, 200.0, &[("t", n(0.18))]);
    let set = g.n("physics.set_velocity", 700.0, 0.0, &[]);
    g.l(input, "vector", target, "a");
    g.l(speed, "value", target, "b");
    g.l(vel, "velocity", lerp, "a");
    g.l(target, "result", lerp, "b");
    g.l(lerp, "result", set, "velocity");
    // Face the movement direction when moving.
    let len = g.n("vec.length", 700.0, 160.0, &[]);
    let moving = g.n("cmp.gt", 900.0, 160.0, &[("b", n(20.0))]);
    let branch = g.n("flow.branch", 940.0, 0.0, &[]);
    let pos = g.n("node.get_position", 940.0, 260.0, &[]);
    let ahead = g.n("math.add", 1160.0, 220.0, &[]);
    let look = g.n("node.look_at", 1200.0, 0.0, &[]);
    g.chain(&[(ev, "then"), (set, "then"), (branch, "true"), (look, "then")]);
    g.l(set, "then", branch, "exec");
    g.l(lerp, "result", len, "vector");
    g.l(len, "length", moving, "a");
    g.l(moving, "result", branch, "condition");
    g.l(pos, "position", ahead, "a");
    g.l(lerp, "result", ahead, "b");
    g.l(ahead, "result", look, "target");

    // Fire: spawn a bullet and give it a velocity variable.
    let fire = g.n("event.action_pressed", 0.0, 420.0, &[("action", s("fire"))]);
    let my_pos = g.n("node.get_position", 0.0, 540.0, &[]);
    let spawn = g.n("node.spawn", 260.0, 420.0, &[("scene", s("scenes/bullet.vscn"))]);
    let rot = g.n("node.get_rotation", 260.0, 600.0, &[]);
    let bs = g.n("var.get", 260.0, 690.0, &[("var", s("bullet_speed"))]);
    let dir = g.n("vec.from_angle", 480.0, 600.0, &[]);
    let setv = g.n("node.set_property", 720.0, 420.0, &[("property", s("vel"))]);
    let shake = g.n("game.shake", 960.0, 420.0, &[("amount", n(0.06))]);
    g.chain(&[(fire, "then"), (spawn, "then"), (setv, "then"), (shake, "then")]);
    g.l(my_pos, "position", spawn, "position");
    g.l(spawn, "node", setv, "node");
    g.l(rot, "degrees", dir, "degrees");
    g.l(bs, "value", dir, "length");
    g.l(dir, "vector", setv, "value");

    // Touching an enemy ends the run.
    let hit = g.n("event.body_entered", 0.0, 820.0, &[]);
    let is_enemy = g.n("node.in_group", 240.0, 920.0, &[("group", s("enemies"))]);
    let br = g.n("flow.branch", 260.0, 820.0, &[]);
    let emit = g.n("game.emit", 480.0, 820.0, &[("name", s("game_over"))]);
    g.chain(&[(hit, "then"), (br, "true"), (emit, "then")]);
    g.l(hit, "then", br, "exec");
    g.l(hit, "other", is_enemy, "node");
    g.l(is_enemy, "result", br, "condition");
    g.json()
}

fn enemy_bp() -> Vec<u8> {
    let mut g = G::new();
    g.var("speed", VType::Number, n(150.0));
    let ev = g.n("event.process", 0.0, 0.0, &[]);
    let me = g.n("node.get_position", 0.0, 120.0, &[]);
    let player = g.n("node.first_in_group", 0.0, 220.0, &[("group", s("player"))]);
    let ppos = g.n("node.get_position", 220.0, 220.0, &[]);
    let dir = g.n("vec.direction", 440.0, 140.0, &[]);
    let speed = g.n("var.get", 440.0, 260.0, &[("var", s("speed"))]);
    let vel = g.n("math.mul", 660.0, 180.0, &[]);
    let set = g.n("physics.set_velocity", 700.0, 0.0, &[]);
    let spin = g.n("node.rotate", 940.0, 0.0, &[]);
    let dt = g.n("math.mul", 940.0, 140.0, &[("b", n(220.0))]);
    g.chain(&[(ev, "then"), (set, "then"), (spin, "then")]);
    g.l(player, "node", ppos, "node");
    g.l(me, "position", dir, "from");
    g.l(ppos, "position", dir, "to");
    g.l(dir, "direction", vel, "a");
    g.l(speed, "value", vel, "b");
    g.l(vel, "result", set, "velocity");
    g.l(ev, "delta", dt, "a");
    g.l(dt, "result", spin, "degrees");
    g.json()
}

fn bullet_bp() -> Vec<u8> {
    let mut g = G::new();
    // Move by the velocity the player assigned (custom property "vel").
    let ev = g.n("event.process", 0.0, 0.0, &[]);
    let vel = g.n("node.get_property", 0.0, 120.0, &[("property", s("vel"))]);
    let step = g.n("math.mul", 240.0, 120.0, &[]);
    let mv = g.n("node.translate", 480.0, 0.0, &[]);
    g.l(ev, "then", mv, "exec");
    g.l(vel, "value", step, "a");
    g.l(ev, "delta", step, "b");
    g.l(step, "result", mv, "offset");
    // Expire after a while.
    let ready = g.n("event.ready", 0.0, 260.0, &[]);
    let delay = g.n("flow.delay", 240.0, 260.0, &[("seconds", n(1.4))]);
    let die = g.n("node.destroy", 480.0, 260.0, &[]);
    g.chain(&[(ready, "then"), (delay, "completed"), (die, "then")]);
    // Hit an enemy: kill both, report the position.
    let hit = g.n("event.body_entered", 0.0, 420.0, &[]);
    let is_enemy = g.n("node.in_group", 0.0, 540.0, &[("group", s("enemies"))]);
    let br = g.n("flow.branch", 240.0, 420.0, &[]);
    let pos = g.n("node.get_position", 240.0, 560.0, &[]);
    let emit = g.n("game.emit", 480.0, 420.0, &[("name", s("enemy_killed"))]);
    let kill_other = g.n("node.destroy", 720.0, 420.0, &[]);
    let kill_self = g.n("node.destroy", 960.0, 420.0, &[]);
    let hitstop = g.n("game.hitstop", 1200.0, 420.0, &[("seconds", n(0.035))]);
    g.chain(&[(hit, "then"), (br, "true"), (emit, "then"), (kill_other, "then"), (kill_self, "then"), (hitstop, "then")]);
    g.l(hit, "then", br, "exec");
    g.l(hit, "other", is_enemy, "node");
    g.l(is_enemy, "result", br, "condition");
    g.l(hit, "other", pos, "node");
    g.l(pos, "position", emit, "payload");
    g.l(hit, "other", kill_other, "node");
    g.json()
}

fn orb_bp() -> Vec<u8> {
    let mut g = G::new();
    // Pulse.
    let ev = g.n("event.process", 0.0, 0.0, &[]);
    let wave = g.n("math.wave", 0.0, 120.0, &[("frequency", n(1.6)), ("amplitude", n(0.18))]);
    let one = g.n("math.add", 220.0, 120.0, &[("b", n(1.0))]);
    let sc = g.n("vec.make", 440.0, 120.0, &[]);
    let set = g.n("node.set_scale", 480.0, 0.0, &[]);
    g.l(ev, "then", set, "exec");
    g.l(wave, "result", one, "a");
    g.l(one, "result", sc, "x");
    g.l(one, "result", sc, "y");
    g.l(sc, "vector", set, "scale");
    // Collected by the player.
    let hit = g.n("event.body_entered", 0.0, 280.0, &[]);
    let is_player = g.n("node.in_group", 0.0, 400.0, &[("group", s("player"))]);
    let br = g.n("flow.branch", 240.0, 280.0, &[]);
    let emit = g.n("game.emit", 480.0, 280.0, &[("name", s("orb"))]);
    let die = g.n("node.destroy", 720.0, 280.0, &[]);
    g.chain(&[(hit, "then"), (br, "true"), (emit, "then"), (die, "then")]);
    g.l(hit, "then", br, "exec");
    g.l(hit, "other", is_player, "node");
    g.l(is_player, "result", br, "condition");
    g.json()
}

fn explosion_bp() -> Vec<u8> {
    let mut g = G::new();
    let ready = g.n("event.ready", 0.0, 0.0, &[]);
    let delay = g.n("flow.delay", 240.0, 0.0, &[("seconds", n(1.0))]);
    let die = g.n("node.destroy", 480.0, 0.0, &[]);
    g.chain(&[(ready, "then"), (delay, "completed"), (die, "then")]);
    // Fade the flash light.
    let ev = g.n("event.process", 0.0, 160.0, &[]);
    let light = g.n("node.get_node", 0.0, 280.0, &[("path", s("Flash"))]);
    let energy = g.n("node.get_property", 240.0, 280.0, &[("property", s("energy"))]);
    let fade = g.n("math.mul", 480.0, 280.0, &[("b", n(0.9))]);
    let set = g.n("node.set_property", 480.0, 160.0, &[("property", s("energy"))]);
    g.l(ev, "then", set, "exec");
    g.l(light, "node", energy, "node");
    g.l(light, "node", set, "node");
    g.l(energy, "value", fade, "a");
    g.l(fade, "result", set, "value");
    g.json()
}

/// Writes template files into `dir`.
pub fn write_to(dir: &std::path::Path, files: &[File]) -> std::io::Result<()> {
    for (path, data) in files {
        let p = dir.join(path);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::assets::MemAssets;
    use crate::game::{Game, LogLevel};
    use crate::script::ScriptRegistry;

    fn run(files: Vec<File>, frames: u32, hold: &[&str]) -> Game {
        let mut assets = MemAssets::default();
        for (p, d) in &files {
            assets.insert(p, d.clone());
        }
        let project = ProjectSettings::from_json(&assets.read_string_lossy("project.vibedot")).unwrap();
        let mut game = Game::new(project, Arc::new(assets), Arc::new(ScriptRegistry::default()));
        assert!(game.start(), "{:?}", game.host.log);
        for k in hold {
            game.host.input.key_down(k);
        }
        for _ in 0..frames {
            game.update(1.0 / 60.0);
            game.host.input.begin_frame();
        }
        let errors: Vec<_> = game.host.log.iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text.clone()).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        game
    }

    impl MemAssets {
        fn read_string_lossy(&self, p: &str) -> String {
            String::from_utf8_lossy(&crate::AssetSource::read(self, p).unwrap()).into_owned()
        }
    }

    #[test]
    fn empty_project_runs() {
        let game = run(empty_project("Test"), 30, &[]);
        assert_eq!(game.tree.get_node_count(), 5);
    }

    #[test]
    fn neon_drift_runs_and_spawns() {
        let game = run(neon_drift(), 240, &["D"]);
        assert!(game.host.log.iter().any(|l| l.text.contains("Neon Drift started")));
        assert!(!game.tree.get_nodes_in_group("enemies").is_empty(), "enemies spawned");
        let main = game.tree.current_scene().unwrap();
        let player = game.tree.get_node(main, "%Player").unwrap();
        assert!(game.tree.global_position(player).x > 100.0, "player flew right");
    }

    #[test]
    fn neon_drift_shooting_kills_enemies() {
        // Hold fire repeatedly while enemies approach.
        let mut assets = MemAssets::default();
        for (p, d) in neon_drift() {
            assets.insert(&p, d);
        }
        let project = ProjectSettings::from_json(&assets.read_string_lossy("project.vibedot")).unwrap();
        let mut game = Game::new(project, Arc::new(assets), Arc::new(ScriptRegistry::default()));
        game.start();
        let mut killed = false;
        for frame in 0..900 {
            if frame % 6 == 0 {
                game.host.input.key_down("J");
            } else {
                game.host.input.key_up("J");
            }
            // Spin to spray bullets around.
            if frame % 40 < 20 {
                game.host.input.key_down("A");
                game.host.input.key_up("D");
            } else {
                game.host.input.key_down("D");
                game.host.input.key_up("A");
            }
            game.update(1.0 / 60.0);
            game.host.input.begin_frame();
            let main = game.tree.current_scene().unwrap();
            if let Some(score) = game.tree.get_node(main, "%Score") {
                let text = game.tree.get(score).unwrap().get("text").unwrap().to_string();
                if text != "SCORE  0" {
                    killed = true;
                    break;
                }
            }
        }
        assert!(killed, "score increased: {:?}", game.host.log.iter().map(|l| &l.text).collect::<Vec<_>>());
    }
}
