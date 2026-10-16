//! Editor application: state, layout and cross-panel requests.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Color32, RichText};
use vd_core::Vec2;
use vd_egui::{GameViewport, theme};
use vd_engine::{AssetSource, DirAssets, Game, LogEntry, LogLevel, ProjectSettings, ScriptRegistry};
use vd_render::DrawList;
use vd_scene::{NodeId, SceneFile};

use crate::doc::{BpDoc, SceneDoc, starter_blueprint};
use crate::{console, files, graph, inspector, manager, scene_dock, viewport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workspace {
    Scene,
    Blueprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Rotate,
    Pan,
}

/// Something a panel wants the editor to do after the UI pass.
#[derive(Clone, Debug)]
pub enum Request {
    OpenScene(String),
    OpenBlueprint(String),
    NewScene(String),
    NewBlueprint {
        path: String,
        attach_to: Option<NodeId>,
    },
    SaveAll,
    Play,
    Stop,
    RunExternal,
    RefreshFiles,
    CloseProject,
    Export,
    InstanceScene {
        path: String,
        at: Vec2,
    },
    AddSprite {
        texture: String,
        at: Vec2,
    },
    DeleteFile(String),
    /// Add an `On Event / Method` handler to `blueprint` (signal stub).
    AddMethodHandler {
        blueprint: String,
        method: String,
    },
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub level: LogLevel,
    pub born: f64,
}

/// State shared by every panel.
#[derive(Default)]
pub struct Shared {
    pub log: Vec<LogEntry>,
    pub toasts: Vec<Toast>,
    pub requests: Vec<Request>,
    pub time: f64,
    /// A pointer gesture (drag) is in progress: postpone undo commits.
    pub gesture: bool,
}

impl Shared {
    pub fn log(&mut self, level: LogLevel, text: impl Into<String>) {
        let text = text.into();
        match level {
            LogLevel::Error => log::error!("{text}"),
            LogLevel::Warn => log::warn!("{text}"),
            LogLevel::Info => log::info!("{text}"),
        }
        self.log.push(LogEntry { level, text, time: self.time });
    }

    pub fn toast(&mut self, level: LogLevel, text: impl Into<String>) {
        let text = text.into();
        self.log(level, text.clone());
        self.toasts.push(Toast { text, level, born: self.time });
    }

    pub fn request(&mut self, r: Request) {
        self.requests.push(r);
    }
}

pub struct Play {
    pub game: Game,
    pub list: DrawList,
    pub paused: bool,
    /// Log lines already copied to the editor console.
    pub log_cursor: usize,
}

#[derive(Clone, Debug)]
pub struct EditorCam {
    pub center: Vec2,
    pub zoom: f32,
}

#[derive(Default)]
pub struct Dialogs {
    pub add_node: Option<scene_dock::AddNodeDialog>,
    pub new_file: Option<files::NewFileDialog>,
    pub export: Option<ExportDialog>,
    pub project_settings: bool,
    pub about: bool,
    pub bytecode: bool,
}

#[derive(Clone, Debug)]
pub struct ExportDialog {
    pub out_dir: String,
    pub platform: crate::export::Platform,
    pub result: Option<Result<String, String>>,
}

pub struct OpenProject {
    pub dir: PathBuf,
    pub settings: ProjectSettings,
    pub assets: Arc<DirAssets>,
    pub files: Vec<String>,
    pub scene: SceneDoc,
    pub blueprints: Vec<BpDoc>,
    pub active_bp: Option<usize>,
    pub workspace: Workspace,
    pub cam: EditorCam,
    pub tool: Tool,
    pub snap: bool,
    pub grid: f32,
    pub show_grid: bool,
    pub show_colliders: bool,
    pub play: Option<Play>,
    pub dialogs: Dialogs,
    pub scene_filter: String,
    pub gizmo: viewport::GizmoState,
    pub graph_ui: graph::GraphUi,
    pub bottom_tab: console::BottomTab,
    pub fps: f32,
}

impl OpenProject {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let settings = ProjectSettings::load_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let assets = Arc::new(DirAssets::new(dir));
        let scene = SceneDoc::load(dir, &settings.main_scene).or_else(|_| {
            // No main scene yet: start with an empty one.
            let mut t = vd_scene::SceneTree::new();
            let root = t.root();
            let main = t
                .add_new_child(root, vd_scene::Node::new("Main", vd_scene::NodeKind::Node2D(Default::default())))
                .map_err(|e| e.to_string())?;
            t.set_current_scene(Some(main));
            Ok::<SceneDoc, String>(SceneDoc::new(&settings.main_scene, t))
        })?;
        let mut p = Self {
            files: assets.list(),
            dir: dir.to_path_buf(),
            cam: EditorCam { center: Vec2::new(settings.window.width as f32 * 0.5, settings.window.height as f32 * 0.5), zoom: 0.8 },
            settings,
            assets,
            scene,
            blueprints: Vec::new(),
            active_bp: None,
            workspace: Workspace::Scene,
            tool: Tool::Select,
            snap: false,
            grid: 32.0,
            show_grid: true,
            show_colliders: true,
            play: None,
            dialogs: Dialogs::default(),
            scene_filter: String::new(),
            gizmo: viewport::GizmoState::default(),
            graph_ui: graph::GraphUi::default(),
            bottom_tab: console::BottomTab::Output,
            fps: 60.0,
        };
        p.frame_scene();
        Ok(p)
    }

    pub fn name(&self) -> &str {
        &self.settings.name
    }

    /// Centers the editor camera on the current camera (or the scene).
    pub fn frame_scene(&mut self) {
        let tree = &mut self.scene.tree;
        tree.update_transforms();
        let cam = tree.all_nodes().into_iter().find(|&id| matches!(tree.get(id).map(|n| &n.kind), Some(vd_scene::NodeKind::Camera2D(_))));
        if let Some(c) = cam {
            self.cam.center = tree.global_position(c);
        }
    }

    pub fn active_bp_mut(&mut self) -> Option<&mut BpDoc> {
        self.active_bp.and_then(|i| self.blueprints.get_mut(i))
    }

    pub fn open_blueprint(&mut self, path: &str, shared: &mut Shared) {
        if let Some(i) = self.blueprints.iter().position(|b| b.path == path) {
            self.active_bp = Some(i);
            self.workspace = Workspace::Blueprint;
            return;
        }
        match BpDoc::load(&self.dir, path) {
            Ok(doc) => {
                self.blueprints.push(doc);
                self.active_bp = Some(self.blueprints.len() - 1);
                self.workspace = Workspace::Blueprint;
            }
            Err(e) => shared.toast(LogLevel::Error, e),
        }
    }

    pub fn save_all(&mut self, shared: &mut Shared) {
        let mut saved = 0;
        if let Err(e) = self.scene.save(&self.dir) {
            shared.toast(LogLevel::Error, format!("save failed: {e}"));
        } else {
            saved += 1;
        }
        for bp in &mut self.blueprints {
            if bp.dirty {
                match bp.save(&self.dir) {
                    Ok(()) => saved += 1,
                    Err(e) => shared.toast(LogLevel::Error, format!("save failed: {e}")),
                }
            }
        }
        if let Err(e) = self.settings.save_dir(&self.dir) {
            shared.toast(LogLevel::Error, format!("project settings: {e}"));
        }
        self.files = self.assets.list();
        shared.toast(LogLevel::Info, format!("Saved {saved} file(s)"));
    }

    pub fn start_play(&mut self, shared: &mut Shared) {
        // Blueprints are read from disk by the game: save dirty ones first.
        for bp in &mut self.blueprints {
            if bp.dirty {
                let _ = bp.save(&self.dir);
            }
        }
        let data = self.scene.tree.pack(self.scene.root());
        let Some(data) = data else { return };
        let mut game = Game::new(self.settings.clone(), self.assets.clone(), Arc::new(ScriptRegistry::default()));
        game.set_scene(&data, Some(self.scene.path.clone()));
        shared.log(LogLevel::Info, format!("▶ Playing {}", self.scene.path));
        self.play = Some(Play { game, list: DrawList::default(), paused: false, log_cursor: 0 });
        self.workspace = Workspace::Scene;
    }

    pub fn stop_play(&mut self, shared: &mut Shared) {
        if self.play.take().is_some() {
            shared.log(LogLevel::Info, "■ Stopped");
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.scene.dirty || self.blueprints.iter().any(|b| b.dirty)
    }
}

pub struct EditorApp {
    pub project: Option<OpenProject>,
    pub manager: manager::ManagerState,
    pub shared: Shared,
    pub viewport: GameViewport,
    last: Instant,
    start: Instant,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            project: None,
            manager: manager::ManagerState::load(),
            shared: Shared::default(),
            viewport: GameViewport::new(Arc::new(|_: &str| None)),
            last: Instant::now(),
            start: Instant::now(),
        };
        if let Some(dir) = open {
            app.open_project(&dir);
        }
        app
    }

    pub fn open_project(&mut self, dir: &Path) {
        match OpenProject::open(dir) {
            Ok(p) => {
                let assets = p.assets.clone();
                self.viewport.set_loader(Arc::new(move |path: &str| assets.read(path)));
                self.viewport.pixel_art = p.settings.render.pixel_art;
                self.manager.remember(dir, p.name());
                self.shared.log(LogLevel::Info, format!("Opened project '{}' ({})", p.name(), dir.display()));
                self.project = Some(p);
            }
            Err(e) => self.shared.toast(LogLevel::Error, e),
        }
    }

    fn handle_requests(&mut self) {
        let requests = std::mem::take(&mut self.shared.requests);
        let shared = &mut self.shared;
        let mut close = false;
        if let Some(p) = &mut self.project {
            for r in requests {
                match r {
                    Request::OpenScene(path) => {
                        if p.scene.dirty {
                            let _ = p.scene.save(&p.dir);
                        }
                        match SceneDoc::load(&p.dir, &path) {
                            Ok(doc) => {
                                p.scene = doc;
                                p.workspace = Workspace::Scene;
                                p.stop_play(shared);
                                p.frame_scene();
                            }
                            Err(e) => shared.toast(LogLevel::Error, e),
                        }
                    }
                    Request::OpenBlueprint(path) => p.open_blueprint(&path, shared),
                    Request::NewScene(path) => {
                        let name = path.rsplit('/').next().unwrap_or("Scene").trim_end_matches(".vscn");
                        let mut t = vd_scene::SceneTree::new();
                        let root = t.root();
                        let pascal: String = name
                            .split(['_', '-', ' '])
                            .map(|w| {
                                let mut c = w.chars();
                                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                            })
                            .collect();
                        let n = t
                            .add_new_child(
                                root,
                                vd_scene::Node::new(
                                    if pascal.is_empty() { "Scene" } else { &pascal },
                                    vd_scene::NodeKind::Node2D(Default::default()),
                                ),
                            )
                            .expect("root");
                        t.set_current_scene(Some(n));
                        match SceneFile::from_tree(&t).save(&p.dir.join(&path)) {
                            Ok(()) => {
                                shared.request(Request::OpenScene(path.clone()));
                                shared.toast(LogLevel::Info, format!("Created {path}"));
                            }
                            Err(e) => shared.toast(LogLevel::Error, e.to_string()),
                        }
                        p.files = p.assets.list();
                    }
                    Request::NewBlueprint { path, attach_to } => {
                        if !p.dir.join(&path).exists()
                            && let Err(e) = starter_blueprint().save(&p.dir.join(&path))
                        {
                            shared.toast(LogLevel::Error, e.to_string());
                            continue;
                        }
                        if let Some(id) = attach_to
                            && let Some(n) = p.scene.tree.get_mut(id)
                        {
                            n.script = Some(path.clone());
                            p.scene.touched = true;
                        }
                        p.files = p.assets.list();
                        p.open_blueprint(&path, shared);
                    }
                    Request::SaveAll => p.save_all(shared),
                    Request::Play => p.start_play(shared),
                    Request::Stop => p.stop_play(shared),
                    Request::RunExternal => {
                        p.save_all(shared);
                        match crate::export::find_player(None, crate::export::Platform::current()) {
                            Ok(player) => match std::process::Command::new(player).arg(&p.dir).spawn() {
                                Ok(_) => shared.log(LogLevel::Info, "Launched game window"),
                                Err(e) => shared.toast(LogLevel::Error, format!("cannot launch player: {e}")),
                            },
                            Err(e) => shared.toast(LogLevel::Error, e.to_string()),
                        }
                    }
                    Request::RefreshFiles => p.files = p.assets.list(),
                    Request::CloseProject => close = true,
                    Request::Export => {
                        p.dialogs.export = Some(ExportDialog {
                            out_dir: p.dir.join("export").display().to_string(),
                            platform: crate::export::Platform::current(),
                            result: None,
                        });
                    }
                    Request::InstanceScene { path, at } => {
                        if let Ok(file) = SceneFile::load(&p.dir.join(&path)) {
                            let parent = p.scene.primary().unwrap_or(p.scene.root());
                            if let Ok(id) = p.scene.tree.instantiate(parent, &file.root) {
                                p.scene.tree.set_global_position(id, at);
                                p.scene.select(id, false);
                                p.scene.touched = true;
                            }
                        }
                    }
                    Request::AddSprite { texture, at } => {
                        let size = image::image_dimensions(p.dir.join(&texture))
                            .map(|(w, h)| Vec2::new(w as f32, h as f32))
                            .unwrap_or(Vec2::splat(64.0));
                        let name = texture.rsplit('/').next().unwrap_or("Sprite").split('.').next().unwrap_or("Sprite").to_owned();
                        let node = vd_scene::Node::new(
                            name,
                            vd_scene::NodeKind::Sprite2D(vd_scene::Sprite2D { texture, size, ..Default::default() }),
                        );
                        let parent = p.scene.root();
                        if let Ok(id) = p.scene.tree.add_new_child(parent, node) {
                            p.scene.tree.set_global_position(id, at);
                            p.scene.select(id, false);
                            p.scene.touched = true;
                        }
                    }
                    Request::DeleteFile(path) => {
                        match std::fs::remove_file(p.dir.join(&path)) {
                            Ok(()) => shared.toast(LogLevel::Info, format!("Deleted {path}")),
                            Err(e) => shared.toast(LogLevel::Error, e.to_string()),
                        }
                        p.blueprints.retain(|b| b.path != path);
                        p.active_bp = if p.blueprints.is_empty() { None } else { Some(0) };
                        p.files = p.assets.list();
                    }
                    Request::AddMethodHandler { blueprint, method } => {
                        p.open_blueprint(&blueprint, shared);
                        if let Some(doc) = p.active_bp_mut() {
                            let exists = doc.bp.nodes.iter().any(|n| n.op == "event.custom" && n.literal("name").to_string() == method);
                            if !exists {
                                let y = doc.bp.nodes.iter().map(|n| n.pos.y).fold(0.0f32, f32::max) + 220.0;
                                let id = doc.bp.add("event.custom", (0.0, y), &[("name", vd_core::Variant::str(&method))]);
                                doc.selection = [id].into();
                                doc.touched = true;
                            }
                        }
                    }
                }
            }
        }
        if close {
            if let Some(p) = &mut self.project
                && p.any_dirty()
            {
                p.save_all(shared);
            }
            self.project = None;
        }
    }
}

fn shortcut(ctx: &egui::Context, key: egui::Key, ctrl: bool, shift: bool) -> bool {
    ctx.input_mut(|i| {
        let m = egui::Modifiers { command: ctrl, ctrl: false, shift, alt: false, mac_cmd: false };
        let _ = m;
        let ok = i.key_pressed(key) && i.modifiers.command == ctrl && i.modifiers.shift == shift;
        if ok {
            i.consume_key(i.modifiers, key);
        }
        ok
    })
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        self.shared.time = (now - self.start).as_secs_f64();
        self.shared.gesture = ctx.input(|i| i.pointer.any_down());

        let Some(p) = &mut self.project else {
            if let Some(dir) = manager::show(ui, &mut self.manager, &mut self.shared) {
                self.open_project(&dir);
            }
            toasts(&ctx, &mut self.shared);
            return;
        };
        p.fps = p.fps * 0.95 + (1.0 / dt.max(1e-4)) * 0.05;

        // Global shortcuts (not while typing into a text field).
        let typing = ctx.memory(|m| m.focused().is_some()) && ctx.egui_wants_keyboard_input();
        if shortcut(&ctx, egui::Key::S, true, false) {
            self.shared.request(Request::SaveAll);
        }
        if shortcut(&ctx, egui::Key::F5, false, false) {
            self.shared.request(if p.play.is_some() { Request::Stop } else { Request::Play });
        }
        if shortcut(&ctx, egui::Key::F6, false, false) {
            self.shared.request(Request::RunExternal);
        }
        if !typing && p.play.is_none() {
            let undo = shortcut(&ctx, egui::Key::Z, true, false);
            let redo = shortcut(&ctx, egui::Key::Z, true, true) || shortcut(&ctx, egui::Key::Y, true, false);
            match p.workspace {
                Workspace::Scene => {
                    if undo {
                        p.scene.undo();
                    }
                    if redo {
                        p.scene.redo();
                    }
                }
                Workspace::Blueprint => {
                    if let Some(doc) = p.active_bp_mut() {
                        if undo {
                            doc.undo();
                        }
                        if redo {
                            doc.redo();
                        }
                    }
                }
            }
        }

        top_bar(ui, p, &mut self.shared);
        status_bar(ui, p, &self.viewport);

        let left = egui::Panel::left("vd_left").resizable(true).default_size(270.0).size_range(200.0..=480.0).frame(dock_frame());
        left.show(ui, |ui| {
            let h = ui.available_height();
            egui::Panel::bottom("vd_files").resizable(true).default_size(h * 0.38).frame(egui::Frame::NONE).show(ui, |ui| {
                files::show(ui, p, &mut self.shared);
            });
            scene_dock::show(ui, p, &mut self.shared);
        });
        egui::Panel::right("vd_right").resizable(true).default_size(320.0).size_range(240.0..=560.0).frame(dock_frame()).show(ui, |ui| {
            match p.workspace {
                Workspace::Scene => inspector::show(ui, p, &mut self.shared),
                Workspace::Blueprint => graph::side_panel(ui, p, &mut self.shared),
            }
        });
        egui::Panel::bottom("vd_bottom").resizable(true).default_size(170.0).size_range(80.0..=500.0).frame(dock_frame()).show(ui, |ui| {
            console::show(ui, p, &mut self.shared);
        });
        egui::CentralPanel::no_frame().show(ui, |ui| match p.workspace {
            Workspace::Scene => viewport::show(ui, frame, p, &mut self.viewport, &mut self.shared, dt),
            Workspace::Blueprint => graph::show(ui, p, &mut self.shared),
        });

        scene_dock::add_node_dialog(&ctx, p, &mut self.shared);
        files::new_file_dialog(&ctx, p, &mut self.shared);
        export_dialog(&ctx, p, &mut self.shared);
        project_settings_dialog(&ctx, p, &mut self.shared);
        about_dialog(&ctx, p);
        graph::bytecode_window(&ctx, p);

        // Commit edits (coalesced per gesture) and pull game logs.
        let gesture = self.shared.gesture;
        p.scene.commit(gesture);
        for bp in &mut p.blueprints {
            bp.commit(gesture);
        }
        if let Some(play) = &mut p.play {
            let new: Vec<LogEntry> = play.game.host.log[play.log_cursor.min(play.game.host.log.len())..].to_vec();
            play.log_cursor = play.game.host.log.len();
            self.shared.log.extend(new);
            if play.game.host.quit_requested {
                self.shared.request(Request::Stop);
            }
        }
        self.handle_requests();
        toasts(&ctx, &mut self.shared);
        let title = match &self.project {
            Some(p) => format!("{}{} — Vibedot", p.name(), if p.any_dirty() { " •" } else { "" }),
            None => "Vibedot".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        ctx.request_repaint();
    }

    fn on_exit(&mut self) {
        if let Some(p) = &mut self.project
            && p.any_dirty()
        {
            p.save_all(&mut self.shared);
        }
    }
}

pub fn dock_frame() -> egui::Frame {
    egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::same(8))
}

fn top_bar(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    egui::Panel::top("vd_top").exact_size(44.0).frame(egui::Frame::NONE.fill(theme::BG).inner_margin(egui::Margin::symmetric(10, 6))).show(
        ui,
        |ui| {
            let playing = p.play.is_some();
            let paused = p.play.as_ref().is_some_and(|pl| pl.paused);
            let name = p.name().to_owned();
            let (_, actions) = egui::Sides::new().height(32.0).show(
                ui,
                |ui| {
                    ui.label(RichText::new("♦").size(22.0).color(theme::ACCENT));
                    ui.label(RichText::new("vibedot").size(17.0).strong().color(theme::TEXT));
                    ui.add_space(8.0);
                    main_menu(ui, p, shared);
                    ui.add_space(16.0);
                    workspace_tabs(ui, p);
                },
                |ui| {
                    let mut actions = Vec::new();
                    if playing {
                        if theme::accent_button(ui, "■  Stop", theme::ERR).on_hover_text("Stop (F5)").clicked() {
                            actions.push(TopAction::Request(Request::Stop));
                        }
                        if ui.button(if paused { "▶ Resume" } else { "⏸ Pause" }).clicked() {
                            actions.push(TopAction::TogglePause);
                        }
                    } else {
                        if theme::accent_button(ui, "▶  Play", theme::ACCENT)
                            .on_hover_text("Play the current scene in the viewport (F5)")
                            .clicked()
                        {
                            actions.push(TopAction::Request(Request::Play));
                        }
                        if ui.button("🗗 Window").on_hover_text("Run the project in a separate window (F6)").clicked() {
                            actions.push(TopAction::Request(Request::RunExternal));
                        }
                    }
                    if ui.button("📦 Export").on_hover_text("Export as a standalone app").clicked() {
                        actions.push(TopAction::Request(Request::Export));
                    }
                    ui.label(RichText::new(&name).color(theme::TEXT_DIM));
                    actions
                },
            );
            for a in actions {
                match a {
                    TopAction::Request(r) => shared.request(r),
                    TopAction::TogglePause => {
                        if let Some(pl) = &mut p.play {
                            pl.paused = !pl.paused;
                        }
                    }
                }
            }
        },
    );
}

enum TopAction {
    Request(Request),
    TogglePause,
}

fn main_menu(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Project", |ui| {
            if ui.button("💾  Save all            Ctrl+S").clicked() {
                shared.request(Request::SaveAll);
            }
            if ui.button("⚙  Project settings…").clicked() {
                p.dialogs.project_settings = true;
            }
            if ui.button("📦  Export app…").clicked() {
                shared.request(Request::Export);
            }
            ui.separator();
            if ui.button("🗁  Open folder").clicked() {
                open_in_file_manager(&p.dir);
            }
            if ui.button("✖  Close project").clicked() {
                shared.request(Request::CloseProject);
            }
        });
        ui.menu_button("Scene", |ui| {
            if ui.button("➕  New scene…").clicked() {
                p.dialogs.new_file = Some(files::NewFileDialog::scene("scenes"));
            }
            if ui.button("🎯  Set as main scene").clicked() {
                p.settings.main_scene = p.scene.path.clone();
                shared.toast(LogLevel::Info, format!("Main scene: {}", p.scene.path));
            }
            if ui.button("➕  Add node…          Ctrl+A").clicked() {
                p.dialogs.add_node = Some(scene_dock::AddNodeDialog::default());
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui.add_enabled(p.scene.history.can_undo(), egui::Button::new("⟲  Undo      Ctrl+Z")).clicked() {
                p.scene.undo();
            }
            if ui.add_enabled(p.scene.history.can_redo(), egui::Button::new("⟳  Redo      Ctrl+Shift+Z")).clicked() {
                p.scene.redo();
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut p.show_grid, "Grid");
            ui.checkbox(&mut p.show_colliders, "Collision shapes");
            ui.checkbox(&mut p.snap, "Snap to grid");
            if ui.button("Frame scene").clicked() {
                p.frame_scene();
            }
            if ui.button("Blueprint bytecode").clicked() {
                p.dialogs.bytecode = true;
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("About Vibedot").clicked() {
                p.dialogs.about = true;
            }
        });
    });
}

fn workspace_tabs(ui: &mut egui::Ui, p: &mut OpenProject) {
    let tabs = [(Workspace::Scene, "🎬  Scene"), (Workspace::Blueprint, "🔷  Blueprints")];
    egui::Frame::NONE.fill(theme::BG_DEEP).corner_radius(10).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (ws, label) in tabs {
                let active = p.workspace == ws;
                let text = RichText::new(label).color(if active { Color32::WHITE } else { theme::TEXT_DIM });
                let b = egui::Button::new(text).fill(if active { theme::SURFACE_HI } else { Color32::TRANSPARENT }).corner_radius(8);
                if ui.add(b).clicked() {
                    p.workspace = ws;
                }
            }
        });
    });
}

fn status_bar(ui: &mut egui::Ui, p: &OpenProject, vp: &GameViewport) {
    egui::Panel::bottom("vd_status")
        .exact_size(24.0)
        .frame(egui::Frame::NONE.fill(theme::BG_DEEP).inner_margin(egui::Margin::symmetric(10, 3)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let small = |t: String| RichText::new(t).small().color(theme::TEXT_DIM);
                let state = if p.play.is_some() {
                    RichText::new("• PLAYING").small().color(theme::OK)
                } else {
                    RichText::new("• EDITING").small().color(theme::ACCENT)
                };
                ui.label(state);
                ui.label(small(format!("{}{}", p.scene.path, if p.scene.dirty { " •" } else { "" })));
                ui.label(small(format!("{} nodes", p.scene.tree.get_node_count().saturating_sub(1))));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(small(format!("Vibedot {}", vd_core::VERSION)));
                    ui.label(small(format!(
                        "{:.0} fps · {} draws · {} inst · {} lights",
                        p.fps, vp.stats.draw_calls, vp.stats.instances, vp.stats.lights
                    )));
                    ui.label(small(format!("zoom {:.0}%", p.cam.zoom * 100.0)));
                });
            });
        });
}

fn toasts(ctx: &egui::Context, shared: &mut Shared) {
    let now = shared.time;
    shared.toasts.retain(|t| now - t.born < 4.0);
    if shared.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::unique("vd_toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0)).show(ctx, |ui| {
        for t in &shared.toasts {
            let color = match t.level {
                LogLevel::Info => theme::ACCENT_2,
                LogLevel::Warn => theme::WARN,
                LogLevel::Error => theme::ERR,
            };
            let alpha = (1.0 - ((now - t.born - 3.2) / 0.8).clamp(0.0, 1.0)) as f32;
            egui::Frame::NONE
                .fill(theme::SURFACE.gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0, color.gamma_multiply(alpha)))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(&t.text).color(theme::TEXT.gamma_multiply(alpha)));
                });
            ui.add_space(6.0);
        }
    });
}

pub fn open_in_file_manager(dir: &Path) {
    let cmd = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

fn export_dialog(ctx: &egui::Context, p: &mut OpenProject, shared: &mut Shared) {
    let Some(mut dlg) = p.dialogs.export.take() else {
        return;
    };
    let d = &mut dlg;
    let mut open = true;
    egui::Window::new("📦  Export app").open(&mut open).collapsible(false).resizable(false).default_width(460.0).show(ctx, |ui| {
        ui.label(RichText::new("Builds a standalone app: the Vibedot player + your project packed into game.vpk.").color(theme::TEXT_DIM));
        ui.add_space(8.0);
        egui::Grid::new("export_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Output folder");
            ui.text_edit_singleline(&mut d.out_dir);
            ui.end_row();
            ui.label("Platform");
            ui.horizontal(|ui| {
                use crate::export::Platform::*;
                for (pl, name) in [(Linux, "🐧 Linux"), (Windows, "⊞ Windows"), (MacOs, "🍎 macOS")] {
                    ui.selectable_value(&mut d.platform, pl, name);
                }
            });
            ui.end_row();
        });
        if d.platform != crate::export::Platform::current() {
            ui.label(
                RichText::new("⚠ Cross-platform export needs a player built for that platform (see build.sh export --target).")
                    .small()
                    .color(theme::WARN),
            );
        }
        ui.add_space(8.0);
        if theme::accent_button(ui, "Export", theme::ACCENT).clicked() {
            p.save_all_quiet();
            let out = PathBuf::from(&d.out_dir);
            d.result = Some(match crate::export::export_project(&p.dir, &out, d.platform, None) {
                Ok(r) => Ok(format!("✔ {} ({} KiB pack)", r.app.display(), r.pack_bytes / 1024)),
                Err(e) => Err(e.to_string()),
            });
            match &d.result {
                Some(Ok(m)) => shared.toast(LogLevel::Info, format!("Exported {m}")),
                Some(Err(e)) => shared.toast(LogLevel::Error, format!("Export failed: {e}")),
                None => {}
            }
        }
        match &d.result {
            Some(Ok(m)) => {
                ui.label(RichText::new(m).color(theme::OK));
                if ui.button("Open folder").clicked() {
                    open_in_file_manager(Path::new(&d.out_dir));
                }
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::ERR));
            }
            None => {}
        }
    });
    if open {
        p.dialogs.export = Some(dlg);
    }
}

impl OpenProject {
    fn save_all_quiet(&mut self) {
        let _ = self.scene.save(&self.dir);
        for bp in &mut self.blueprints {
            let _ = bp.save(&self.dir);
        }
        let _ = self.settings.save_dir(&self.dir);
    }
}

fn project_settings_dialog(ctx: &egui::Context, p: &mut OpenProject, shared: &mut Shared) {
    if !p.dialogs.project_settings {
        return;
    }
    let mut open = true;
    egui::Window::new("⚙  Project settings").open(&mut open).default_width(520.0).show(ctx, |ui| {
        let s = &mut p.settings;
        egui::Grid::new("ps").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut s.name);
            ui.end_row();
            ui.label("Main scene");
            egui::ComboBox::from_id_salt("main_scene").selected_text(&s.main_scene).show_ui(ui, |ui| {
                for f in p.files.iter().filter(|f| f.ends_with(".vscn")) {
                    ui.selectable_value(&mut s.main_scene, f.clone(), f);
                }
            });
            ui.end_row();
            ui.label("Window size");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.window.width).range(160..=7680));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut s.window.height).range(120..=4320));
            });
            ui.end_row();
            ui.label("Window");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.window.resizable, "Resizable");
                ui.checkbox(&mut s.window.fullscreen, "Fullscreen");
                ui.checkbox(&mut s.window.vsync, "V-Sync");
            });
            ui.end_row();
            ui.label("Gravity");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.physics.gravity.x).speed(1.0));
                ui.add(egui::DragValue::new(&mut s.physics.gravity.y).speed(1.0));
            });
            ui.end_row();
            ui.label("Physics ticks / s");
            ui.add(egui::DragValue::new(&mut s.physics.ticks_per_second).range(10..=240));
            ui.end_row();
            ui.label("Pixel art filtering");
            ui.checkbox(&mut s.render.pixel_art, "");
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("Input map").strong());
        ui.label(
            RichText::new("Keys: A–Z, Num0–Num9, Space, Enter, Escape, ArrowLeft…, F1–F12, MouseLeft/Right/Middle")
                .small()
                .color(theme::TEXT_DIM),
        );
        let mut remove = None;
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for (action, keys) in s.input.iter_mut() {
                ui.horizontal(|ui| {
                    ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(action).monospace()));
                    let mut joined = keys.join(", ");
                    if ui.add(egui::TextEdit::singleline(&mut joined).desired_width(220.0)).changed() {
                        *keys = joined.split(',').map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()).collect();
                    }
                    if ui.small_button("🗑").clicked() {
                        remove = Some(action.clone());
                    }
                });
            }
        });
        if let Some(r) = remove {
            s.input.remove(&r);
        }
        ui.horizontal(|ui| {
            let id = ui.scope_id().with("new_action");
            let mut name: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
            ui.text_edit_singleline(&mut name);
            if ui.button("➕ Add action").clicked() && !name.trim().is_empty() {
                s.input.insert(name.trim().to_owned(), Vec::new());
                name.clear();
            }
            ui.data_mut(|d| d.insert_temp(id, name));
        });
        ui.add_space(6.0);
        if theme::accent_button(ui, "Save settings", theme::ACCENT).clicked() {
            match s.save_dir(&p.dir) {
                Ok(()) => shared.toast(LogLevel::Info, "Project settings saved"),
                Err(e) => shared.toast(LogLevel::Error, e.to_string()),
            }
        }
    });
    if !open {
        p.dialogs.project_settings = false;
    }
}

fn about_dialog(ctx: &egui::Context, p: &mut OpenProject) {
    if !p.dialogs.about {
        return;
    }
    let mut open = true;
    egui::Window::new("About").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("♦ vibedot").size(30.0).color(theme::ACCENT));
            ui.label(format!("version {}", vd_core::VERSION));
            ui.add_space(6.0);
            ui.label("A Rust game engine following Godot's architecture,");
            ui.label("with visual Blueprints and an HDR 2D renderer.");
            ui.add_space(6.0);
            ui.label(
                RichText::new("Godot Engine is © Juan Linietsky, Ariel Manzur and contributors (MIT).").small().color(theme::TEXT_DIM),
            );
        });
    });
    if !open {
        p.dialogs.about = false;
    }
}
