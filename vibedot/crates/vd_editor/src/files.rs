//! FileSystem dock: project files as a folder tree; double-click to open,
//! drag images/scenes into the viewport, context menu to create files.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use vd_egui::theme;
use vd_engine::LogLevel;

use crate::app::{OpenProject, Request, Shared};

/// Drag payload: a project-relative file path.
#[derive(Clone, Debug)]
pub struct DragFile(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewKind {
    Scene,
    Blueprint,
}

pub struct NewFileDialog {
    pub kind: NewKind,
    pub folder: String,
    pub name: String,
    focus: bool,
}

impl NewFileDialog {
    pub fn scene(folder: &str) -> Self {
        Self { kind: NewKind::Scene, folder: folder.into(), name: "new_scene".into(), focus: false }
    }
    pub fn blueprint(folder: &str) -> Self {
        Self { kind: NewKind::Blueprint, folder: folder.into(), name: "new_blueprint".into(), focus: false }
    }
}

fn icon(path: &str) -> (&'static str, egui::Color32) {
    match path.rsplit('.').next().unwrap_or("") {
        "vscn" => ("🎬", theme::ACCENT_2),
        "vbp" => ("🔷", theme::ACCENT),
        "png" | "jpg" | "jpeg" => ("🖼", theme::OK),
        "vibedot" => ("⚙", theme::WARN),
        "wav" | "ogg" => ("🔊", theme::TEXT_DIM),
        _ => ("📄", theme::TEXT_DIM),
    }
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<String>,
}

fn build(files: &[String]) -> Dir {
    let mut root = Dir::default();
    for f in files {
        let mut cur = &mut root;
        let parts: Vec<&str> = f.split('/').collect();
        for d in &parts[..parts.len() - 1] {
            cur = cur.dirs.entry((*d).to_owned()).or_default();
        }
        cur.files.push(f.clone());
    }
    root
}

pub fn show(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    ui.horizontal(|ui| {
        theme::section(ui, "🗁", "FileSystem");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("⟳").on_hover_text("Refresh").clicked() {
                shared.request(Request::RefreshFiles);
            }
            ui.menu_button("➕", |ui| {
                if ui.button("🎬 New scene…").clicked() {
                    p.dialogs.new_file = Some(NewFileDialog::scene("scenes"));
                    ui.close();
                }
                if ui.button("🔷 New blueprint…").clicked() {
                    p.dialogs.new_file = Some(NewFileDialog::blueprint("blueprints"));
                    ui.close();
                }
            });
        });
    });
    ui.add_space(2.0);
    let tree = build(&p.files);
    egui::ScrollArea::vertical().id_salt("files_scroll").auto_shrink([false, false]).show(ui, |ui| {
        dir_ui(ui, p, shared, &tree, "", 0);
    });
}

fn dir_ui(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared, dir: &Dir, prefix: &str, depth: usize) {
    for (name, sub) in &dir.dirs {
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let id = ui.make_persistent_id(("dir", &path));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth == 0)
            .show_header(ui, |ui| {
                let r = ui.label(RichText::new(format!("📁 {name}")).color(theme::TEXT));
                r.context_menu(|ui| {
                    if ui.button("🎬 New scene here…").clicked() {
                        p.dialogs.new_file = Some(NewFileDialog::scene(&path));
                        ui.close();
                    }
                    if ui.button("🔷 New blueprint here…").clicked() {
                        p.dialogs.new_file = Some(NewFileDialog::blueprint(&path));
                        ui.close();
                    }
                    if ui.button("🗁 Show in file manager").clicked() {
                        crate::app::open_in_file_manager(&p.dir.join(&path));
                        ui.close();
                    }
                });
            })
            .body(|ui| dir_ui(ui, p, shared, sub, &path, depth + 1));
    }
    for f in &dir.files {
        let name = f.rsplit('/').next().unwrap_or(f);
        let (ic, color) = icon(f);
        let current = *f == p.scene.path || p.active_bp.and_then(|i| p.blueprints.get(i)).is_some_and(|b| b.path == *f);
        let text = RichText::new(format!("{ic}  {name}")).color(if current { color } else { theme::TEXT });
        let r = ui.add(egui::Label::new(text).sense(egui::Sense::click_and_drag()).selectable(false));
        if f.ends_with(".png") || f.ends_with(".vscn") {
            r.dnd_set_drag_payload(DragFile(f.clone()));
        }
        if r.double_clicked() {
            if f.ends_with(".vscn") {
                shared.request(Request::OpenScene(f.clone()));
            } else if f.ends_with(".vbp") {
                shared.request(Request::OpenBlueprint(f.clone()));
            } else if f.ends_with(".vibedot") {
                p.dialogs.project_settings = true;
            }
        }
        let r = if f.ends_with(".png") {
            r.on_hover_ui(|ui| {
                let uri = format!("file://{}", p.dir.join(f).display());
                ui.add(egui::Image::new(uri).max_size(egui::vec2(160.0, 160.0)));
                ui.label(RichText::new("Drag into the viewport to create a Sprite2D").small());
            })
        } else {
            r.on_hover_text(f)
        };
        r.context_menu(|ui| {
            if f.ends_with(".vscn") && ui.button("Open").clicked() {
                shared.request(Request::OpenScene(f.clone()));
                ui.close();
            }
            if f.ends_with(".vbp") && ui.button("Edit").clicked() {
                shared.request(Request::OpenBlueprint(f.clone()));
                ui.close();
            }
            if ui.button("📋 Copy path").clicked() {
                ui.ctx().copy_text(f.clone());
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("🗑 Delete").color(theme::ERR)).clicked() {
                if *f == p.scene.path || *f == p.settings.main_scene {
                    shared.toast(LogLevel::Warn, "Can't delete the open/main scene");
                } else {
                    shared.request(Request::DeleteFile(f.clone()));
                }
                ui.close();
            }
        });
    }
}

pub fn new_file_dialog(ctx: &egui::Context, p: &mut OpenProject, shared: &mut Shared) {
    let Some(d) = &mut p.dialogs.new_file else {
        return;
    };
    let mut open = true;
    let mut create = false;
    let (title, ext) = match d.kind {
        NewKind::Scene => ("🎬  New scene", "vscn"),
        NewKind::Blueprint => ("🔷  New blueprint", "vbp"),
    };
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("nf").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Folder");
                ui.text_edit_singleline(&mut d.folder);
                ui.end_row();
                ui.label("Name");
                let r = ui.text_edit_singleline(&mut d.name);
                if !d.focus {
                    r.request_focus();
                    d.focus = true;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    create = true;
                }
                ui.end_row();
            });
            ui.label(RichText::new(format!("» {}/{}.{ext}", d.folder.trim_matches('/'), d.name)).small().color(theme::TEXT_DIM));
            if theme::accent_button(ui, "Create", theme::ACCENT).clicked() {
                create = true;
            }
        });
    if create {
        let name = d.name.trim().trim_end_matches(&format!(".{ext}")).to_owned();
        let path = format!("{}/{}.{ext}", d.folder.trim_matches('/'), name);
        if p.files.contains(&path) {
            shared.toast(LogLevel::Warn, format!("{path} already exists"));
        } else {
            shared.request(match d.kind {
                NewKind::Scene => Request::NewScene(path),
                NewKind::Blueprint => Request::NewBlueprint { path, attach_to: None },
            });
            p.dialogs.new_file = None;
        }
    } else if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        p.dialogs.new_file = None;
    }
}
