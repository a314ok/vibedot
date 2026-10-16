//! # vd_egui
//!
//! Glue between the engine and the windowing/UI layer (eframe + egui on
//! wgpu), shared by the editor and the standalone player:
//!
//! * [`GameViewport`] renders a [`DrawList`] into an offscreen wgpu texture
//!   that egui displays as an image (no extra copies to the CPU);
//! * [`feed_input`] translates egui keyboard/mouse events into engine input;
//! * [`theme`] is the redesigned Vibedot look.
#![forbid(unsafe_code)]

pub mod theme;

use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu;
use vd_core::Vec2;
use vd_engine::InputState;
use vd_render::{AssetLoader, DrawList, RenderStats, Renderer, wgpu};

/// Name of an egui key as used in input maps (`"A"`, `"Space"`, `"ArrowLeft"`).
pub fn key_name(key: egui::Key) -> String {
    format!("{key:?}")
}

pub fn pointer_name(b: egui::PointerButton) -> &'static str {
    match b {
        egui::PointerButton::Primary => "MouseLeft",
        egui::PointerButton::Secondary => "MouseRight",
        egui::PointerButton::Middle => "MouseMiddle",
        egui::PointerButton::Extra1 => "MouseExtra1",
        egui::PointerButton::Extra2 => "MouseExtra2",
    }
}

/// Feeds this frame's egui events into `input`. `rect` is the on-screen
/// rectangle of the game image and `pixels_per_point` converts to pixels.
/// Pointer presses outside the rect are ignored; key events only when
/// `keyboard_focus` is set.
pub fn feed_input(ctx: &egui::Context, rect: egui::Rect, input: &mut InputState, keyboard_focus: bool) {
    let ppp = ctx.pixels_per_point();
    ctx.input(|i| {
        if let Some(p) = i.pointer.latest_pos() {
            let local = (p - rect.min) * ppp;
            input.mouse_screen = Vec2::new(local.x, local.y);
        }
        for e in &i.events {
            match e {
                egui::Event::Key { key, pressed, repeat, .. } if keyboard_focus => {
                    if *repeat {
                        continue;
                    }
                    let name = key_name(*key);
                    if *pressed {
                        input.key_down(&name);
                    } else {
                        input.key_up(&name);
                    }
                }
                egui::Event::PointerButton { pos, button, pressed, .. } => {
                    let name = pointer_name(*button);
                    if *pressed && rect.contains(*pos) {
                        input.key_down(name);
                    } else if !*pressed {
                        input.key_up(name);
                    }
                }
                egui::Event::Text(t) if keyboard_focus => input.text.push_str(t),
                egui::Event::WindowFocused(false) => input.release_all(),
                _ => {}
            }
        }
    });
}

/// Offscreen render target shown through egui.
pub struct GameViewport {
    renderer: Option<Renderer>,
    target: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    texture_id: Option<egui::TextureId>,
    loader: AssetLoader,
    pub pixel_art: bool,
    pub stats: RenderStats,
}

/// Display-encoded 8-bit target: egui samples textures as gamma values.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl GameViewport {
    pub fn new(loader: AssetLoader) -> Self {
        Self { renderer: None, target: None, texture_id: None, loader, pixel_art: false, stats: RenderStats::default() }
    }

    /// Replaces the asset loader (e.g. after opening another project).
    pub fn set_loader(&mut self, loader: AssetLoader) {
        self.loader = loader;
        self.renderer = None;
    }

    pub fn invalidate_textures(&mut self) {
        if let Some(r) = &mut self.renderer {
            r.invalidate_all_textures();
        }
    }

    pub fn renderer_mut(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }

    /// Renders `list` at `size` pixels and returns the egui texture to draw.
    pub fn render(&mut self, rs: &egui_wgpu::RenderState, list: &DrawList, size: (u32, u32)) -> egui::TextureId {
        let size = (size.0.clamp(1, 8192), size.1.clamp(1, 8192));
        let renderer = self.renderer.get_or_insert_with(|| Renderer::new(&rs.device, &rs.queue, Arc::clone(&self.loader)));
        renderer.pixel_art = self.pixel_art;
        let recreate = self.target.as_ref().is_none_or(|(_, _, s)| *s != size);
        if recreate {
            let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vd game viewport"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TARGET_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let mut egui_renderer = rs.renderer.write();
            match self.texture_id {
                Some(id) => egui_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, wgpu::FilterMode::Linear, id),
                None => {
                    self.texture_id = Some(egui_renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear));
                }
            }
            self.target = Some((tex, view, size));
        }
        let (_, view, _) = self.target.as_ref().expect("created above");
        let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vd viewport") });
        self.stats = renderer.render(&mut encoder, list, view, TARGET_FORMAT, size);
        rs.queue.submit([encoder.finish()]);
        self.texture_id.expect("registered above")
    }

    /// Paints the game image filling `rect`.
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        if let Some(id) = self.texture_id {
            ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
    }
}

/// Size in physical pixels of an egui rect.
pub fn rect_pixels(ctx: &egui::Context, rect: egui::Rect) -> (u32, u32) {
    let ppp = ctx.pixels_per_point();
    ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32)
}

/// Minimal stderr logger (`VIBEDOT_LOG=debug|info|warn|error`).
pub fn init_logger() {
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, m: &log::Metadata<'_>) -> bool {
            m.level() <= log::max_level()
        }
        fn log(&self, r: &log::Record<'_>) {
            // Keep third-party crates quiet unless they warn.
            let ours = r.target().starts_with("vd_") || r.target().starts_with("vibedot");
            if self.enabled(r.metadata()) && (ours || r.level() <= log::Level::Warn) {
                eprintln!("[{:5}] {}", r.level(), r.args());
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    let level = match std::env::var("VIBEDOT_LOG").as_deref() {
        Ok("debug") => log::LevelFilter::Debug,
        Ok("warn") => log::LevelFilter::Warn,
        Ok("error") => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    };
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}
