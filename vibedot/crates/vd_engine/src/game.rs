//! The running game: owns the scene tree, script instances and physics,
//! and advances them frame by frame in Godot's order:
//!
//! 1. input events (`_input`, blueprint `On Key/Action/Mouse` events)
//! 2. fixed-step physics (`_physics_process`, bodies, contact signals)
//! 3. internal processing (timers, particles, sprite animation)
//! 4. `_process` in priority order (blueprint `On Process`, Rust scripts)
//! 5. latent `Delay` continuations
//! 6. queued notifications / signal calls are pumped after every step
//! 7. `queue_free` deletions, then deferred scene changes
//! 8. camera update and transform propagation

use std::collections::HashMap;
use std::sync::Arc;

use vd_blueprint::{EventKind, Host, Instance, Program, compile};
use vd_core::{Affine2, Color, Rect2, Rng, StringName, Variant, Vec2, vec2};
use vd_physics::{Body, BodyKind, Shape, World};
use vd_render::{DrawList, View2D};
use vd_scene::{BodyType, CollisionShape, NodeData, NodeId, NodeKind, Notification, Particle, SceneFile, SceneTree, TreeEvent};

use crate::assets::{AssetSource, normalize_path};
use crate::draw::draw_tree;
use crate::input::{InputMap, InputState};
use crate::project::ProjectSettings;
use crate::script::{NodeScript, ScriptCtx, ScriptRegistry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: LogLevel,
    pub text: String,
    pub time: f64,
}

#[derive(Clone, Debug)]
enum SceneRequest {
    Change(String),
    Reload,
}

#[derive(Clone, Debug)]
struct CustomEvent {
    name: StringName,
    payload: Variant,
    sender: NodeId,
    target: Option<NodeId>,
}

/// Everything blueprints and scripts may touch besides the tree.
pub struct HostState {
    pub assets: Arc<dyn AssetSource>,
    pub project: ProjectSettings,
    pub input_map: InputMap,
    pub input: InputState,
    pub time: f64,
    pub rng: Rng,
    pub view: View2D,
    /// Viewport size in pixels.
    pub viewport: Vec2,
    pub log: Vec<LogEntry>,
    pub quit_requested: bool,
    pub time_scale: f32,
    hitstop: f32,
    flash: (Color, f32, f32),
    shake: f32,
    programs: HashMap<String, Option<Arc<Program>>>,
    scenes: HashMap<String, Option<Arc<NodeData>>>,
    events: Vec<CustomEvent>,
    pending_scene: Option<SceneRequest>,
}

impl HostState {
    fn new(project: ProjectSettings, assets: Arc<dyn AssetSource>) -> Self {
        let input_map = InputMap::new(&project.input);
        let viewport = vec2(project.window.width as f32, project.window.height as f32);
        Self {
            assets,
            input_map,
            input: InputState::default(),
            time: 0.0,
            rng: Rng::new(0x5EED),
            view: View2D { center: viewport * 0.5, zoom: 1.0, rotation: 0.0 },
            viewport,
            log: Vec::new(),
            quit_requested: false,
            time_scale: 1.0,
            hitstop: 0.0,
            flash: (Color::TRANSPARENT, 0.0, 0.0),
            shake: 0.0,
            programs: HashMap::new(),
            scenes: HashMap::new(),
            events: Vec::new(),
            pending_scene: None,
            project,
        }
    }

    fn push_log(&mut self, level: LogLevel, text: String) {
        match level {
            LogLevel::Info => log::info!("{text}"),
            LogLevel::Warn => log::warn!("{text}"),
            LogLevel::Error => log::error!("{text}"),
        }
        self.log.push(LogEntry { level, text, time: self.time });
        if self.log.len() > 2000 {
            self.log.drain(..500);
        }
    }

    pub fn info(&mut self, text: &str) {
        self.push_log(LogLevel::Info, text.to_owned());
    }

    pub fn warn(&mut self, text: &str) {
        self.push_log(LogLevel::Warn, text.to_owned());
    }

    pub fn error(&mut self, text: &str) {
        self.push_log(LogLevel::Error, text.to_owned());
    }

    pub fn is_action_pressed(&self, a: StringName) -> bool {
        self.input_map.is_action_pressed(&self.input, a)
    }

    pub fn is_action_just_pressed(&self, a: StringName) -> bool {
        self.input_map.is_action_just_pressed(&self.input, a)
    }

    pub fn mouse_world(&self) -> Vec2 {
        self.view.screen_to_world(self.viewport, self.input.mouse_screen)
    }

    /// Visible world rectangle.
    pub fn view_rect(&self) -> Rect2 {
        let half = self.viewport * 0.5 / self.view.zoom.max(1e-4);
        Rect2::from_min_max(self.view.center - half, self.view.center + half)
    }

    /// Compiles (and caches) a blueprint. Errors are logged once.
    pub fn program(&mut self, path: &str) -> Option<Arc<Program>> {
        let key = normalize_path(path);
        if let Some(p) = self.programs.get(&key) {
            return p.clone();
        }
        let result = match self.assets.read_string(&key) {
            None => Err(format!("blueprint '{key}' not found")),
            Some(json) => match vd_blueprint::Blueprint::from_json(&json) {
                Err(e) => Err(format!("blueprint '{key}': {e}")),
                Ok(bp) => compile(&bp).map(Arc::new).map_err(|errs| {
                    let list: Vec<String> = errs.iter().map(ToString::to_string).collect();
                    format!("blueprint '{key}' has errors:\n  {}", list.join("\n  "))
                }),
            },
        };
        let p = match result {
            Ok(p) => Some(p),
            Err(e) => {
                self.error(&e);
                None
            }
        };
        self.programs.insert(key, p.clone());
        p
    }

    /// Loads (and caches) a scene file.
    pub fn scene_data(&mut self, path: &str) -> Option<Arc<NodeData>> {
        let key = normalize_path(path);
        if let Some(s) = self.scenes.get(&key) {
            return s.clone();
        }
        let s = match self.assets.read_string(&key).map(|j| SceneFile::from_json(&j)) {
            Some(Ok(f)) => Some(Arc::new(f.root)),
            Some(Err(e)) => {
                self.error(&format!("scene '{key}': {e}"));
                None
            }
            None => {
                self.error(&format!("scene '{key}' not found"));
                None
            }
        };
        self.scenes.insert(key, s.clone());
        s
    }

    /// Drops cached blueprints and scenes (hot reload).
    pub fn clear_caches(&mut self) {
        self.programs.clear();
        self.scenes.clear();
    }

    pub fn spawn(&mut self, tree: &mut SceneTree, path: &str, parent: Option<NodeId>, pos: Vec2) -> Option<NodeId> {
        let data = self.scene_data(path)?;
        let parent = parent.filter(|&p| tree.contains(p)).unwrap_or_else(|| tree.scene_root());
        match tree.instantiate(parent, &data) {
            Ok(id) => {
                tree.set_global_position(id, pos);
                Some(id)
            }
            Err(e) => {
                self.error(&format!("cannot spawn '{path}': {e}"));
                None
            }
        }
    }
}

impl Host for HostState {
    fn time(&self) -> f64 {
        self.time
    }
    fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }
    fn action_pressed(&self, a: StringName) -> bool {
        self.is_action_pressed(a)
    }
    fn action_just_pressed(&self, a: StringName) -> bool {
        self.is_action_just_pressed(a)
    }
    fn mouse_world(&self) -> Vec2 {
        HostState::mouse_world(self)
    }
    fn spawn_scene(&mut self, tree: &mut SceneTree, path: &str, parent: Option<NodeId>, pos: Vec2) -> Option<NodeId> {
        self.spawn(tree, path, parent, pos)
    }
    fn print(&mut self, msg: &str) {
        self.info(msg);
    }
    fn emit_event(&mut self, name: StringName, payload: Variant, sender: NodeId, target: Option<NodeId>) {
        self.events.push(CustomEvent { name, payload, sender, target });
    }
    fn change_scene(&mut self, path: Option<&str>) {
        self.pending_scene = Some(path.map_or(SceneRequest::Reload, |p| SceneRequest::Change(p.to_owned())));
    }
    fn camera_shake(&mut self, amount: f32) {
        self.shake += amount;
    }
    fn hit_stop(&mut self, seconds: f32) {
        self.hitstop = self.hitstop.max(seconds);
    }
    fn set_time_scale(&mut self, scale: f32) {
        self.time_scale = scale.clamp(0.0, 10.0);
    }
    fn flash(&mut self, color: Color, duration: f32) {
        self.flash = (color, duration.max(0.001), duration.max(0.001));
    }
    fn quit(&mut self) {
        self.quit_requested = true;
    }
}

/// A running game.
pub struct Game {
    pub tree: SceneTree,
    pub host: HostState,
    blueprints: HashMap<NodeId, Instance>,
    scripts: HashMap<NodeId, Box<dyn NodeScript>>,
    registry: Arc<ScriptRegistry>,
    physics: World,
    accumulator: f32,
    /// Path of the scene currently running (for "Restart Scene").
    pub scene_path: Option<String>,
    /// Frames simulated so far.
    pub frame: u64,
}

const BUILTIN_METHODS: &[&str] = &["queue_free", "hide", "show", "start", "stop", "burst", "toggle_visible"];

impl Game {
    pub fn new(project: ProjectSettings, assets: Arc<dyn AssetSource>, registry: Arc<ScriptRegistry>) -> Self {
        let gravity = project.physics.gravity;
        Self {
            tree: SceneTree::new(),
            host: HostState::new(project, assets),
            blueprints: HashMap::new(),
            scripts: HashMap::new(),
            registry,
            physics: World::new(gravity),
            accumulator: 0.0,
            scene_path: None,
            frame: 0,
        }
    }

    /// Loads the project's main scene.
    pub fn start(&mut self) -> bool {
        let main = self.host.project.main_scene.clone();
        self.change_scene_now(&main)
    }

    /// Replaces the current scene with `data` (used by play-in-editor).
    pub fn set_scene(&mut self, data: &NodeData, path: Option<String>) {
        if let Some(old) = self.tree.current_scene() {
            let _ = self.tree.free(old);
        }
        self.cleanup_instances();
        self.physics = World::new(self.host.project.physics.gravity);
        let root = self.tree.root();
        match self.tree.instantiate(root, data) {
            Ok(id) => self.tree.set_current_scene(Some(id)),
            Err(e) => self.host.error(&format!("cannot instantiate scene: {e}")),
        }
        self.scene_path = path;
        self.tree.update_transforms();
        self.pump();
        self.update_camera(0.0);
    }

    pub fn change_scene_now(&mut self, path: &str) -> bool {
        match self.host.scene_data(path) {
            Some(data) => {
                self.set_scene(&data, Some(normalize_path(path)));
                true
            }
            None => false,
        }
    }

    pub fn registry(&self) -> &ScriptRegistry {
        &self.registry
    }

    pub fn blueprint_instance(&self, id: NodeId) -> Option<&Instance> {
        self.blueprints.get(&id)
    }

    // ---------------------------------------------------------------- frame

    /// Advances the game by `real_dt` seconds. Feed input into
    /// `self.host.input` before calling, and call `host.input.begin_frame()`
    /// before feeding the next frame's events.
    pub fn update(&mut self, real_dt: f32) {
        let real_dt = real_dt.clamp(0.0, 0.25);
        let mut dt = real_dt * self.host.time_scale;
        if self.host.hitstop > 0.0 {
            self.host.hitstop -= real_dt;
            dt = 0.0;
        }
        self.host.time += f64::from(dt);
        self.frame += 1;

        self.dispatch_input();
        self.pump();

        let step = 1.0 / self.host.project.physics.ticks_per_second.max(1) as f32;
        self.accumulator += dt;
        let mut steps = 0;
        while self.accumulator >= step && steps < 8 {
            self.physics_step(step);
            self.accumulator -= step;
            steps += 1;
        }
        if steps == 8 {
            self.accumulator = 0.0; // spiral-of-death guard
        }

        self.update_timers(dt);
        self.update_particles(dt);
        self.update_sprites(dt);
        self.pump();

        for id in self.tree.process() {
            self.call_process(id, dt);
        }
        self.pump();

        let ids: Vec<NodeId> = self.blueprints.keys().copied().collect();
        for id in ids {
            if self.tree.can_process(id) {
                self.with_blueprint(id, |inst, tree, host| inst.update_delays(dt, tree, host));
            }
        }
        self.pump();

        self.tree.flush_queued();
        self.cleanup_instances();
        if let Some(req) = self.host.pending_scene.take() {
            match req {
                SceneRequest::Change(p) => {
                    self.change_scene_now(&p);
                }
                SceneRequest::Reload => {
                    if let Some(p) = self.scene_path.clone() {
                        self.host.scenes.remove(&p);
                        self.change_scene_now(&p);
                    }
                }
            }
        }

        let (c, dur, rem) = self.host.flash;
        if rem > 0.0 {
            self.host.flash = (c, dur, (rem - real_dt).max(0.0));
        }
        self.tree.update_transforms();
        self.update_camera(real_dt);
    }

    /// Fills `out` with this frame's draw commands.
    pub fn draw(&self, out: &mut DrawList) {
        out.clear();
        out.view = self.host.view;
        out.time = self.host.time as f32;
        draw_tree(&self.tree, out, out.time);
        let (c, dur, rem) = self.host.flash;
        if rem > 0.0 {
            out.flash = c.with_alpha(c.a * rem / dur);
        }
    }

    // ---------------------------------------------------------------- dispatch helpers

    fn with_blueprint(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut Instance, &mut SceneTree, &mut HostState) -> Result<(), vd_blueprint::RuntimeError>,
    ) {
        let Some(mut inst) = self.blueprints.remove(&id) else {
            return;
        };
        if let Err(e) = f(&mut inst, &mut self.tree, &mut self.host) {
            let path = self.tree.path_of(id);
            let at = e.node.map(|n| format!(" (blueprint node #{})", n.0)).unwrap_or_default();
            self.host.error(&format!("{path}: {}{at}", e.message));
        }
        if self.tree.contains(id) {
            self.blueprints.insert(id, inst);
        }
    }

    fn with_script(&mut self, id: NodeId, f: impl FnOnce(&mut dyn NodeScript, &mut ScriptCtx<'_>)) {
        let Some(mut s) = self.scripts.remove(&id) else {
            return;
        };
        {
            let mut cx = ScriptCtx { tree: &mut self.tree, host: &mut self.host, node: id };
            f(s.as_mut(), &mut cx);
        }
        if self.tree.contains(id) {
            self.scripts.insert(id, s);
        }
    }

    fn trigger(&mut self, id: NodeId, kind: EventKind, key: StringName, args: &[Variant]) {
        if self.blueprints.get(&id).is_some_and(|i| i.handles(kind)) {
            self.with_blueprint(id, |inst, tree, host| inst.trigger(kind, key, args, tree, host).map(|_| ()));
        }
    }

    fn cleanup_instances(&mut self) {
        let tree = &self.tree;
        self.blueprints.retain(|id, _| tree.contains(*id));
        self.scripts.retain(|id, _| tree.contains(*id));
    }

    /// Dispatches queued tree events and custom events until quiescent.
    fn pump(&mut self) {
        for _ in 0..64 {
            let events = self.tree.take_events();
            let customs = std::mem::take(&mut self.host.events);
            if events.is_empty() && customs.is_empty() {
                return;
            }
            for e in events {
                match e {
                    TreeEvent::Notification(id, what) => self.notification(id, what),
                    TreeEvent::Call { source, target, method, args, .. } => self.call_method(source, target, method, &args),
                }
            }
            for ev in customs {
                let targets: Vec<NodeId> = match ev.target {
                    Some(t) => vec![t],
                    None => self
                        .tree
                        .all_nodes()
                        .into_iter()
                        .filter(|id| self.blueprints.contains_key(id) || self.scripts.contains_key(id))
                        .collect(),
                };
                for t in targets {
                    self.call_method(ev.sender, t, ev.name, std::slice::from_ref(&ev.payload));
                }
            }
        }
        self.host.warn("event pump limit reached (signals calling each other in a loop?)");
    }

    fn notification(&mut self, id: NodeId, what: Notification) {
        if !self.tree.contains(id) {
            return;
        }
        match what {
            Notification::EnterTree => {
                self.attach_script(id);
                self.with_script(id, |s, cx| s.enter_tree(cx));
            }
            Notification::Ready => {
                self.with_script(id, |s, cx| s.ready(cx));
                self.trigger(id, EventKind::Ready, StringName::EMPTY, &[]);
            }
            Notification::ExitTree => self.with_script(id, |s, cx| s.exit_tree(cx)),
            Notification::Paused | Notification::Unpaused => {}
        }
    }

    fn attach_script(&mut self, id: NodeId) {
        if self.blueprints.contains_key(&id) || self.scripts.contains_key(&id) {
            return;
        }
        let Some(script) = self.tree.get(id).and_then(|n| n.script.clone()) else {
            return;
        };
        if script.is_empty() {
            return;
        }
        if let Some(name) = script.strip_prefix("rust:") {
            match self.registry.create(name) {
                Some(s) => {
                    let n = self.tree.get_mut(id).expect("checked");
                    if s.wants_process() {
                        n.set_process(true);
                    }
                    if s.wants_physics_process() {
                        n.set_physics_process(true);
                    }
                    self.scripts.insert(id, s);
                }
                None => self.host.error(&format!("unknown Rust script '{name}' on {}", self.tree.path_of(id))),
            }
            return;
        }
        let Some(program) = self.host.program(&script) else {
            return;
        };
        let node = self.tree.get_mut(id).expect("checked");
        if program.has_event(EventKind::Process) {
            node.set_process(true);
        }
        let overrides = node.vars.clone();
        let inst = Instance::new(program, id, |name| overrides.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()));
        self.blueprints.insert(id, inst);
    }

    fn call_method(&mut self, source: NodeId, target: NodeId, method: StringName, args: &[Variant]) {
        if !self.tree.contains(target) {
            return;
        }
        // Built-in methods (a tiny subset of Godot's bound methods).
        if BUILTIN_METHODS.contains(&method.as_str()) && !self.blueprints.contains_key(&target) {
            let tree = &mut self.tree;
            match method.as_str() {
                "queue_free" => tree.queue_free(target),
                "hide" => tree.get_mut(target).expect("checked").visible = false,
                "show" => tree.get_mut(target).expect("checked").visible = true,
                "toggle_visible" => {
                    let n = tree.get_mut(target).expect("checked");
                    n.visible = !n.visible;
                }
                "start" | "stop" | "burst" => match &mut tree.get_mut(target).expect("checked").kind {
                    NodeKind::Timer(t) => {
                        t.running = method.as_str() == "start";
                        t.time_left = t.wait_time;
                        t.initialized = true;
                    }
                    NodeKind::Particles2D(p) => {
                        p.burst_pending = method.as_str() != "stop";
                        p.emitting = method.as_str() == "start";
                    }
                    _ => {}
                },
                _ => {}
            }
            return;
        }
        let payload = args.first().cloned().unwrap_or_default();
        let sender = Variant::Object(SceneTree::object_id(source));
        let all = Variant::Array(Arc::new(args.to_vec()));
        self.trigger(target, EventKind::Custom, method, &[payload, sender, all]);
        let m = method.as_str().to_owned();
        let args = args.to_vec();
        self.with_script(target, |s, cx| {
            s.call(cx, &m, &args);
        });
    }

    fn call_process(&mut self, id: NodeId, dt: f32) {
        self.with_script(id, |s, cx| s.process(cx, dt));
        self.trigger(id, EventKind::Process, StringName::EMPTY, &[Variant::Number(f64::from(dt))]);
    }

    // ---------------------------------------------------------------- input

    fn dispatch_input(&mut self) {
        let presses = self.host.input.press_order.clone();
        let listeners: Vec<NodeId> =
            self.tree.all_nodes().into_iter().filter(|&id| self.blueprints.contains_key(&id) && self.tree.can_process(id)).collect();
        if listeners.is_empty() {
            return;
        }
        let mut fired_actions: Vec<StringName> = Vec::new();
        for key in &presses {
            let mouse = matches!(key.as_str(), "MouseLeft" | "MouseRight" | "MouseMiddle");
            let mouse_pos = Variant::Vec2(self.host.mouse_world());
            for &id in &listeners {
                self.trigger(id, EventKind::KeyPressed, *key, &[]);
                if mouse {
                    let button = StringName::new(key.as_str().trim_start_matches("Mouse"));
                    self.trigger(id, EventKind::MousePressed, button, std::slice::from_ref(&mouse_pos));
                }
            }
            for &action in self.host.input_map.actions_for_key(*key).to_vec().iter() {
                if !fired_actions.contains(&action) && self.host.is_action_just_pressed(action) {
                    fired_actions.push(action);
                    for &id in &listeners {
                        self.trigger(id, EventKind::ActionPressed, action, &[]);
                    }
                }
            }
        }
        // Releases.
        let mut released: Vec<StringName> = Vec::new();
        for (action, _) in self.host.project.input.clone() {
            let a = StringName::new(&action);
            if self.host.input_map.is_action_just_released(&self.host.input, a) {
                released.push(a);
            }
        }
        for a in released {
            for &id in &listeners {
                self.trigger(id, EventKind::ActionReleased, a, &[]);
            }
        }
    }

    // ---------------------------------------------------------------- physics

    fn physics_step(&mut self, dt: f32) {
        for id in self.tree.physics_process() {
            self.with_script(id, |s, cx| s.physics_process(cx, dt));
        }
        self.pump();
        self.tree.update_transforms();
        self.physics.gravity = self.host.project.physics.gravity;
        self.physics.bodies.clear();
        for id in self.tree.all_nodes() {
            if !self.tree.can_process(id) {
                continue;
            }
            let node = self.tree.get(id).expect("listed");
            let NodeKind::Body2D(b) = &node.kind else {
                continue;
            };
            let gxf = node.global_transform();
            let scale = Vec2::new(gxf.matrix2.x_axis.length(), gxf.matrix2.y_axis.length());
            let shape = match b.shape {
                CollisionShape::Rect => Shape::Rect { half: b.size * scale * 0.5 },
                CollisionShape::Circle => Shape::Circle { radius: b.radius * scale.max_element() },
            };
            let kind = match b.body_type {
                BodyType::Static => BodyKind::Static,
                BodyType::Character => BodyKind::Character,
                BodyType::Rigid => BodyKind::Rigid,
                BodyType::Area => BodyKind::Area,
            };
            let mut body = Body::new(id.to_bits(), kind, shape, gxf.translation);
            body.vel = b.velocity;
            body.mass = b.mass;
            body.bounce = b.bounce;
            body.friction = b.friction;
            body.gravity_scale = b.gravity_scale;
            body.layer = b.layer;
            body.mask = b.mask;
            self.physics.bodies.push(body);
        }
        let events = self.physics.step(dt);
        let view = self.host.view_rect();
        let bodies = std::mem::take(&mut self.physics.bodies);
        for body in &bodies {
            let Some(id) = NodeId::from_bits(body.key).filter(|&id| self.tree.contains(id)) else {
                continue;
            };
            if !matches!(body.kind, BodyKind::Character | BodyKind::Rigid) {
                continue;
            }
            let mut pos = body.pos;
            let clamp = matches!(&self.tree.get(id).expect("exists").kind, NodeKind::Body2D(b) if b.clamp_to_view);
            if clamp {
                let half = body.shape.aabb(Vec2::ZERO).size * 0.5;
                pos = pos.clamp(view.min + half, view.max() - half);
            }
            self.tree.set_global_position(id, pos);
            if let NodeKind::Body2D(b) = &mut self.tree.get_mut(id).expect("exists").kind {
                b.velocity = body.vel;
                b.on_floor = body.on_floor;
            }
        }
        self.physics.bodies = bodies;
        for (a, b) in events.entered {
            self.contact(a, b, true);
            self.contact(b, a, true);
        }
        for (a, b) in events.exited {
            self.contact(a, b, false);
            self.contact(b, a, false);
        }
        self.pump();
    }

    fn contact(&mut self, me: u64, other: u64, entered: bool) {
        let (Some(me), Some(other)) = (NodeId::from_bits(me), NodeId::from_bits(other)) else {
            return;
        };
        if !self.tree.contains(me) || !self.tree.contains(other) {
            return;
        }
        let o = Variant::Object(SceneTree::object_id(other));
        let (signal, kind) = if entered { ("body_entered", EventKind::BodyEntered) } else { ("body_exited", EventKind::BodyExited) };
        self.tree.emit_signal(me, signal, std::slice::from_ref(&o));
        self.trigger(me, kind, StringName::EMPTY, &[o]);
    }

    // ---------------------------------------------------------------- internal processing

    fn update_timers(&mut self, dt: f32) {
        let mut fired = Vec::new();
        for id in self.tree.all_nodes() {
            if !self.tree.can_process(id) {
                continue;
            }
            let Some(node) = self.tree.get_mut(id) else {
                continue;
            };
            let NodeKind::Timer(t) = &mut node.kind else {
                continue;
            };
            if !t.initialized {
                t.initialized = true;
                t.running = t.autostart;
                t.time_left = t.wait_time;
            }
            if !t.running {
                continue;
            }
            t.time_left -= dt;
            if t.time_left <= 0.0 {
                if t.one_shot {
                    t.running = false;
                    t.time_left = 0.0;
                } else {
                    // Keep the remainder so long timers do not drift.
                    t.time_left += t.wait_time.max(0.001);
                    if t.time_left <= 0.0 {
                        t.time_left = t.wait_time;
                    }
                }
                fired.push(id);
            }
        }
        for id in fired {
            let name = StringName::new(&self.tree.get(id).map(|n| n.name.clone()).unwrap_or_default());
            let obj = Variant::Object(SceneTree::object_id(id));
            self.tree.emit_signal(id, "timeout", &[]);
            self.trigger(id, EventKind::Timeout, name, std::slice::from_ref(&obj));
            if let Some(p) = self.tree.get_parent(id) {
                self.trigger(p, EventKind::Timeout, name, std::slice::from_ref(&obj));
            }
        }
    }

    fn update_particles(&mut self, dt: f32) {
        let ids = self.tree.all_nodes();
        for id in ids {
            if !self.tree.can_process(id) {
                continue;
            }
            let Some(node) = self.tree.get(id) else {
                continue;
            };
            if !matches!(node.kind, NodeKind::Particles2D(_)) {
                continue;
            }
            let gxf = node.global_transform();
            let rot = vd_core::Transform2D::from_affine(&gxf).rotation;
            let rng = &mut self.host.rng;
            let NodeKind::Particles2D(p) = &mut self.tree.get_mut(id).expect("exists").kind else { unreachable!() };
            if !p.started {
                p.started = true;
                if p.burst > 0 {
                    p.burst_pending = true;
                }
                if p.one_shot && p.emitting {
                    p.burst = p.burst.max((p.rate * p.lifetime) as u32);
                    p.burst_pending = true;
                    p.emitting = false;
                }
            }
            let mut spawn = 0u32;
            if p.burst_pending {
                spawn += p.burst;
                p.burst_pending = false;
            }
            if p.emitting && dt > 0.0 {
                p.accumulator += p.rate * dt;
                let whole = p.accumulator.floor();
                p.accumulator -= whole;
                spawn += whole as u32;
            }
            let cap = p.max_particles as usize;
            for _ in 0..spawn {
                if p.particles.len() >= cap {
                    break;
                }
                let dir = p.direction + rng.range(-1.0, 1.0) * p.spread + if p.world_space { rot } else { 0.0 };
                let speed = rng.range(p.speed_min, p.speed_max.max(p.speed_min));
                let offset = rng.in_circle(p.emit_radius);
                let (pos, vel) = if p.world_space {
                    (gxf.transform_point2(offset), Vec2::from_angle(dir) * speed)
                } else {
                    (offset, Vec2::from_angle(dir) * speed)
                };
                let life = p.lifetime * (1.0 - p.lifetime_random * rng.next_f32());
                p.particles.push(Particle {
                    pos,
                    vel,
                    age: 0.0,
                    life: life.max(0.01),
                    spin: rng.range(-3.0, 3.0),
                    angle: rng.range(0.0, std::f32::consts::TAU),
                    size_jitter: 1.0 - 0.35 * rng.next_f32(),
                });
            }
            let damp = (1.0 - p.damping * dt).max(0.0);
            let g = p.gravity;
            p.particles.retain_mut(|q| {
                q.age += dt;
                q.vel = (q.vel + g * dt) * damp;
                q.pos += q.vel * dt;
                q.angle += q.spin * dt;
                q.age < q.life
            });
        }
    }

    fn update_sprites(&mut self, dt: f32) {
        for id in self.tree.all_nodes() {
            if !self.tree.can_process(id) {
                continue;
            }
            if let Some(NodeKind::Sprite2D(s)) = self.tree.get_mut(id).map(|n| &mut n.kind)
                && s.anim_fps > 0.0
            {
                s.anim_time += dt;
                let total = (s.hframes.max(1) * s.vframes.max(1)).max(1);
                s.frame = ((s.anim_time * s.anim_fps) as u32) % total;
            }
        }
    }

    fn update_camera(&mut self, dt: f32) {
        let cam = self
            .tree
            .all_nodes()
            .into_iter()
            .find(|&id| matches!(self.tree.get(id).map(|n| &n.kind), Some(NodeKind::Camera2D(c)) if c.current));
        let viewport = self.host.viewport;
        let Some(cam) = cam else {
            self.host.view = View2D { center: viewport * 0.5, zoom: 1.0, rotation: 0.0 };
            return;
        };
        let target = self.tree.global_position(cam);
        let shake_add = std::mem::take(&mut self.host.shake);
        let rng = &mut self.host.rng;
        let NodeKind::Camera2D(c) = &mut self.tree.get_mut(cam).expect("exists").kind else { unreachable!() };
        c.trauma = (c.trauma + shake_add).clamp(0.0, 1.0);
        let pos = match (c.smoothed, c.smoothing > 0.0) {
            (Some(prev), true) => prev.lerp(target, vd_core::math::damp_factor(c.smoothing, dt)),
            _ => target,
        };
        c.smoothed = Some(pos);
        let shake = rng.in_circle(1.0) * c.shake_strength * c.trauma * c.trauma;
        c.trauma = (c.trauma - c.shake_decay * dt).max(0.0);
        self.host.view = View2D { center: pos + shake, zoom: c.zoom.max(0.01), rotation: 0.0 };
    }

    /// World-space bounding box of everything visible (for "frame all").
    pub fn bounds(&self) -> Option<Rect2> {
        let mut r: Option<Rect2> = None;
        for id in self.tree.all_nodes() {
            let n = self.tree.get(id)?;
            if n.kind.is_2d() {
                let p = n.global_position();
                let b = Rect2::from_center_size(p, Vec2::splat(64.0));
                r = Some(r.map_or(b, |r| r.union(&b)));
            }
        }
        r
    }
}

/// Transform helper used by tests and tools.
pub fn affine_pos(a: &Affine2) -> Vec2 {
    a.translation
}
