//! `vibedot-player`: runs a Vibedot game.
//!
//! ```text
//! vibedot-player                     # game.vpk next to the executable, or ./project.vibedot
//! vibedot-player path/to/project     # a project directory
//! vibedot-player game.vpk            # an exported pack
//! vibedot-player <game> --screenshot out.png [--frames 120] [--size 1280x720] [--hold D,Space]
//! ```
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use vd_egui::{GameViewport, feed_input, rect_pixels, theme};
use vd_engine::{AssetSource, DirAssets, Game, PROJECT_FILE, PackAssets, ProjectSettings, ScriptRegistry};
use vd_render::DrawList;

struct Loaded {
    project: ProjectSettings,
    assets: Arc<dyn AssetSource>,
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Finds the game to run: explicit path, exported pack beside the binary
/// (also `../Resources` inside a macOS bundle), or the current directory.
fn locate(arg: Option<&str>) -> Result<Loaded, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(a) = arg {
        candidates.push(PathBuf::from(a));
    } else {
        if let Some(dir) = exe_dir() {
            candidates.push(dir.join("game.vpk"));
            candidates.push(dir.join("../Resources/game.vpk"));
        }
        candidates.push(PathBuf::from("."));
    }
    for c in candidates {
        if c.is_file() && c.extension().is_some_and(|e| e == "vpk") {
            let pack = PackAssets::open(&c).map_err(|e| format!("{}: {e}", c.display()))?;
            let json = pack.read_string(PROJECT_FILE).ok_or_else(|| format!("{}: missing {PROJECT_FILE}", c.display()))?;
            let project = ProjectSettings::from_json(&json).map_err(|e| e.to_string())?;
            return Ok(Loaded { project, assets: Arc::new(pack) });
        }
        let dir = if c.is_file() { c.parent().map(Path::to_path_buf).unwrap_or_default() } else { c.clone() };
        if dir.join(PROJECT_FILE).is_file() {
            let project = ProjectSettings::load_dir(&dir).map_err(|e| e.to_string())?;
            return Ok(Loaded { project, assets: Arc::new(DirAssets::new(dir)) });
        }
    }
    Err("No game found. Pass a project folder or a .vpk file, or place game.vpk next to the player.".into())
}

struct PlayerApp {
    game: Option<Game>,
    error: Option<String>,
    viewport: GameViewport,
    list: DrawList,
    last: Instant,
    show_stats: bool,
    fullscreen: bool,
    fps: f32,
}

impl PlayerApp {
    fn new(cc: &eframe::CreationContext<'_>, loaded: Result<Loaded, String>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (game, error, loader): (Option<Game>, Option<String>, vd_render::AssetLoader) = match loaded {
            Ok(l) => {
                let assets = Arc::clone(&l.assets);
                let loader: vd_render::AssetLoader = Arc::new(move |p: &str| assets.read(p));
                let pixel_art = l.project.render.pixel_art;
                let mut game = Game::new(l.project, l.assets, Arc::new(ScriptRegistry::default()));
                let err = (!game.start()).then(|| {
                    let last = game.host.log.last().map(|e| e.text.clone()).unwrap_or_default();
                    format!("Could not start the main scene.\n{last}")
                });
                let _ = pixel_art;
                (Some(game), err, loader)
            }
            Err(e) => (None, Some(e), Arc::new(|_: &str| None)),
        };
        let mut viewport = GameViewport::new(loader);
        viewport.pixel_art = game.as_ref().is_some_and(|g| g.host.project.render.pixel_art);
        Self { game, error, viewport, list: DrawList::default(), last: Instant::now(), show_stats: false, fullscreen: false, fps: 60.0 }
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        self.fps = self.fps * 0.95 + (1.0 / dt.max(1e-4)) * 0.05;

        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.show_stats = !self.show_stats;
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            if let Some(err) = &self.error {
                ui.painter().rect_filled(rect, 0.0, theme::BG_DEEP);
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(48.0)), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(rect.height() * 0.25);
                        ui.label(egui::RichText::new("♦ Vibedot").size(42.0).color(theme::ACCENT));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(err).size(16.0).color(theme::TEXT));
                    });
                });
                return;
            }
            let Some(game) = &mut self.game else { return };
            let Some(rs) = frame.wgpu_render_state() else {
                return;
            };
            let size = rect_pixels(&ctx, rect);
            game.host.viewport = vd_core::vec2(size.0 as f32, size.1 as f32);
            game.host.input.begin_frame();
            feed_input(&ctx, rect, &mut game.host.input, true);
            game.update(dt);
            game.draw(&mut self.list);
            self.viewport.render(rs, &self.list, size);
            self.viewport.paint(ui, rect);
            if self.show_stats {
                let s = self.viewport.stats;
                let txt = format!(
                    "{:.0} FPS  |  {} nodes  |  {} draw calls  |  {} instances  |  {} lights",
                    self.fps,
                    game.tree.get_node_count(),
                    s.draw_calls,
                    s.instances,
                    s.lights
                );
                ui.painter().text(rect.min + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, txt, egui::FontId::monospace(13.0), theme::OK);
            }
            if game.host.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ctx.request_repaint();
    }
}

fn screenshot(loaded: Loaded, out: &Path, frames: u32, size: (u32, u32), hold: &[String]) -> Result<(), String> {
    let (device, queue) = vd_render::headless_device().ok_or("no GPU adapter available")?;
    let assets = Arc::clone(&loaded.assets);
    let mut renderer = vd_render::Renderer::new(&device, &queue, Arc::new(move |p: &str| assets.read(p)));
    renderer.pixel_art = loaded.project.render.pixel_art;
    let mut game = Game::new(loaded.project, loaded.assets, Arc::new(ScriptRegistry::default()));
    game.host.viewport = vd_core::vec2(size.0 as f32, size.1 as f32);
    if !game.start() {
        return Err("cannot start main scene".into());
    }
    for k in hold {
        game.host.input.key_down(k);
    }
    for _ in 0..frames {
        game.update(1.0 / 60.0);
        game.host.input.begin_frame();
    }
    for l in &game.host.log {
        eprintln!("[game] {}", l.text);
    }
    let mut list = DrawList::default();
    game.draw(&mut list);
    let px = vd_render::render_to_rgba(&mut renderer, &list, size);
    image::save_buffer(out, &px, size.0, size.1, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
    eprintln!("saved {} ({} draw calls, {} instances)", out.display(), renderer.stats.draw_calls, renderer.stats.instances);
    Ok(())
}

fn main() {
    vd_egui::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let positional = args.iter().enumerate().find(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[*i - 1].starts_with("--")));
    let loaded = locate(positional.map(|(_, a)| a.as_str()));

    if let Some(out) = flag("--screenshot") {
        let frames = flag("--frames").and_then(|f| f.parse().ok()).unwrap_or(60);
        let size = flag("--size")
            .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
            .unwrap_or((1280, 720));
        let hold: Vec<String> = flag("--hold").map(|h| h.split(',').map(str::to_owned).collect()).unwrap_or_default();
        let result = loaded.and_then(|l| screenshot(l, Path::new(&out), frames, size, &hold));
        if let Err(e) = result {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let (title, w, h, resizable, fullscreen, vsync) = match &loaded {
        Ok(l) => {
            let win = &l.project.window;
            (l.project.name.clone(), win.width, win.height, win.resizable, win.fullscreen, win.vsync)
        }
        Err(_) => ("Vibedot Player".to_owned(), 960, 540, true, false, true),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([w as f32, h as f32])
            .with_resizable(resizable)
            .with_fullscreen(fullscreen),
        ..Default::default()
    };
    let mut options = options;
    options.wgpu_options.surface.present_mode =
        if vsync { vd_render::wgpu::PresentMode::AutoVsync } else { vd_render::wgpu::PresentMode::AutoNoVsync };
    if let Err(e) = eframe::run_native(&title, options, Box::new(move |cc| Ok(Box::new(PlayerApp::new(cc, loaded))))) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
