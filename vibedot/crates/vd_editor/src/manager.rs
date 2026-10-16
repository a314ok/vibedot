//! Project manager: recent projects, create from template, open by path.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use vd_egui::theme;
use vd_engine::LogLevel;

use crate::app::Shared;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recent {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Empty,
    NeonDrift,
}

pub struct ManagerState {
    pub recent: Vec<Recent>,
    pub new_name: String,
    pub new_dir: String,
    pub template: Template,
    pub open_path: String,
}

pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("VIBEDOT_CONFIG") {
        return PathBuf::from(d);
    }
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or_default().join("Vibedot")
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join("Library/Application Support/Vibedot")
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join(".config"))
            .join("vibedot")
    }
}

fn documents_dir() -> PathBuf {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join("VibedotProjects")
}

impl ManagerState {
    pub fn load() -> Self {
        let recent = std::fs::read_to_string(config_dir().join("recent.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<Recent>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.path.join(vd_engine::PROJECT_FILE).is_file())
            .collect();
        Self {
            recent,
            new_name: "My Game".into(),
            new_dir: documents_dir().display().to_string(),
            template: Template::NeonDrift,
            open_path: String::new(),
        }
    }

    pub fn remember(&mut self, dir: &Path, name: &str) {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        self.recent.retain(|r| r.path != dir);
        self.recent.insert(0, Recent { name: name.to_owned(), path: dir });
        self.recent.truncate(12);
        let cfg = config_dir();
        let _ = std::fs::create_dir_all(&cfg);
        let _ = std::fs::write(cfg.join("recent.json"), serde_json::to_string_pretty(&self.recent).unwrap_or_default());
    }
}

/// Creates a project from a template. Fails if the folder already has one.
pub fn create_project(dir: &Path, name: &str, template: Template) -> Result<(), String> {
    if dir.join(vd_engine::PROJECT_FILE).exists() {
        return Err(format!("{} already contains a project", dir.display()));
    }
    let mut files = match template {
        Template::Empty => vd_engine::templates::empty_project(name),
        Template::NeonDrift => vd_engine::templates::neon_drift(),
    };
    // Apply the chosen name.
    for (path, data) in &mut files {
        if path == vd_engine::PROJECT_FILE
            && let Ok(mut s) = vd_engine::ProjectSettings::from_json(&String::from_utf8_lossy(data))
        {
            s.name = name.to_owned();
            *data = s.to_json().into_bytes();
        }
    }
    vd_engine::templates::write_to(dir, &files).map_err(|e| e.to_string())
}

/// Shows the manager; returns a project directory to open.
pub fn show(ui: &mut egui::Ui, st: &mut ManagerState, shared: &mut Shared) -> Option<PathBuf> {
    let mut open = None;
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        // Backdrop: soft accent glows.
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, theme::BG_DEEP);
        for (c, pos, r) in [
            (theme::ACCENT, rect.left_top() + egui::vec2(rect.width() * 0.2, rect.height() * 0.15), 420.0),
            (theme::ACCENT_2, rect.right_bottom() - egui::vec2(rect.width() * 0.15, rect.height() * 0.2), 380.0),
        ] {
            for i in 0..24 {
                let t = i as f32 / 24.0;
                painter.circle_filled(pos, r * (1.0 - t), c.gamma_multiply(0.012 + t * 0.006));
            }
        }
        let content = rect.shrink2(egui::vec2((rect.width() - 980.0).max(32.0) / 2.0, 48.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("♦").size(46.0).color(theme::ACCENT));
                ui.vertical(|ui| {
                    ui.label(RichText::new("vibedot").size(40.0).strong().color(Color32::WHITE));
                    ui.label(RichText::new("Rust game engine · Godot architecture · Blueprints · HDR 2D").color(theme::TEXT_DIM));
                });
            });
            ui.add_space(28.0);
            ui.columns(2, |cols| {
                card(&mut cols[0], "Recent projects", |ui| {
                    if st.recent.is_empty() {
                        ui.label(RichText::new("No recent projects yet - create one on the right.").color(theme::TEXT_DIM));
                    }
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        for r in st.recent.clone() {
                            let resp = egui::Frame::NONE.fill(theme::SURFACE).corner_radius(8).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(&r.name).strong().size(15.0));
                                ui.label(RichText::new(r.path.display().to_string()).small().color(theme::TEXT_DIM));
                            });
                            let resp = resp.response.interact(egui::Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_stroke(resp.rect, 8, egui::Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
                            }
                            if resp.clicked() {
                                open = Some(r.path.clone());
                            }
                            ui.add_space(6.0);
                        }
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("Open by path").strong());
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut st.open_path).hint_text("/path/to/project").desired_width(300.0));
                        if ui.button("Open").clicked() {
                            let p = PathBuf::from(st.open_path.trim());
                            if p.join(vd_engine::PROJECT_FILE).is_file() {
                                open = Some(p);
                            } else {
                                shared.toast(LogLevel::Error, format!("No {} in {}", vd_engine::PROJECT_FILE, p.display()));
                            }
                        }
                    });
                });
                card(&mut cols[1], "New project", |ui| {
                    egui::Grid::new("np").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut st.new_name);
                        ui.end_row();
                        ui.label("Location");
                        ui.text_edit_singleline(&mut st.new_dir);
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Template").strong());
                    for (t, title, desc) in [
                        (Template::NeonDrift, "🌌  Neon Drift", "Complete arcade game: ship, enemies, shooting, particles, bloom - all in Blueprints"),
                        (Template::Empty, "▫  Empty", "One scene with a camera and an environment"),
                    ] {
                        let selected = st.template == t;
                        let r = egui::Frame::NONE
                            .fill(if selected { theme::SURFACE_HI } else { theme::SURFACE })
                            .stroke(egui::Stroke::new(1.0, if selected { theme::ACCENT } else { theme::BORDER }))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(title).strong());
                                ui.label(RichText::new(desc).small().color(theme::TEXT_DIM));
                            });
                        if r.response.interact(egui::Sense::click()).clicked() {
                            st.template = t;
                        }
                        ui.add_space(4.0);
                    }
                    ui.add_space(10.0);
                    if theme::accent_button(ui, "✨  Create project", theme::ACCENT).clicked() {
                        let folder = crate::export::slug(&st.new_name);
                        let dir = PathBuf::from(st.new_dir.trim()).join(folder);
                        match create_project(&dir, st.new_name.trim(), st.template) {
                            Ok(()) => open = Some(dir),
                            Err(e) => shared.toast(LogLevel::Error, e),
                        }
                    }
                });
            });
        });
    });
    open
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(theme::PANEL.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(14)
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(18.0).strong());
            ui.add_space(10.0);
            add(ui);
        });
}
