//! Scene dock: the node hierarchy with search, drag & drop reparenting,
//! visibility toggles and a context menu; plus the "Add Node" dialog.

use eframe::egui::{self, Color32, RichText};
use vd_egui::theme;
use vd_engine::LogLevel;
use vd_scene::{Node, NodeId, NodeKind, SceneFile};

use crate::app::{OpenProject, Request, Shared};

#[derive(Default)]
pub struct AddNodeDialog {
    pub search: String,
    pub focus_set: bool,
}

/// Payload for drag & drop inside the dock.
#[derive(Clone, Copy)]
struct DragNode(NodeId);

pub fn show(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    ui.horizontal(|ui| {
        theme::section(ui, "🎬", "Scene");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("➕").on_hover_text("Add child node (Ctrl+A)").clicked() {
                p.dialogs.add_node = Some(AddNodeDialog::default());
            }
            if ui.button("🔗").on_hover_text("Instance a scene as child").clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("instance_menu"), true));
            }
        });
    });
    ui.add(egui::TextEdit::singleline(&mut p.scene_filter).hint_text("🔍 Filter nodes").desired_width(f32::INFINITY));
    ui.add_space(4.0);

    let show_instance = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("instance_menu")).unwrap_or(false));
    if show_instance {
        egui::Frame::NONE.fill(theme::SURFACE).corner_radius(6).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.label(RichText::new("Instance scene:").small().color(theme::TEXT_DIM));
            for f in p.files.iter().filter(|f| f.ends_with(".vscn") && **f != p.scene.path) {
                if ui.button(format!("🎬 {f}")).clicked() {
                    shared.request(Request::InstanceScene { path: f.clone(), at: p.cam.center });
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("instance_menu"), false));
                }
            }
            if ui.small_button("Cancel").clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("instance_menu"), false));
            }
        });
    }

    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) && !ui.ctx().egui_wants_keyboard_input() {
        p.dialogs.add_node = Some(AddNodeDialog::default());
    }

    let mut actions: Vec<Action> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let root = p.scene.root();
        row(ui, p, root, 0, &mut actions);
        // Drop on empty space: move to the scene root.
        let rest = ui.allocate_response(ui.available_size().max(egui::vec2(0.0, 40.0)), egui::Sense::click());
        if let Some(payload) = rest.dnd_release_payload::<DragNode>() {
            actions.push(Action::Reparent(payload.0, root));
        }
        if rest.clicked() {
            p.scene.selection.clear();
        }
    });

    for a in actions {
        apply(p, shared, a);
    }

    // Keyboard shortcuts for the selection.
    if !ui.ctx().egui_wants_keyboard_input() && p.play.is_none() && p.workspace == crate::app::Workspace::Scene {
        let (del, dup, f2) = ui.input(|i| (i.key_pressed(egui::Key::Delete), i.modifiers.command && i.key_pressed(egui::Key::D), i.key_pressed(egui::Key::F2)));
        if del {
            for id in p.scene.selection.clone() {
                apply(p, shared, Action::Delete(id));
            }
        }
        if dup && let Some(id) = p.scene.primary() {
            apply(p, shared, Action::Duplicate(id));
        }
        if f2 && let Some(id) = p.scene.primary() {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("rename_node"), id.to_bits()));
        }
    }
}

enum Action {
    Select(NodeId, bool),
    Toggle(NodeId),
    Reparent(NodeId, NodeId),
    MoveUp(NodeId),
    MoveDown(NodeId),
    Delete(NodeId),
    Duplicate(NodeId),
    ToggleVisible(NodeId),
    Rename(NodeId, String),
    AddChild(NodeId),
    AttachBlueprint(NodeId),
    SaveBranch(NodeId),
    MakeRoot(NodeId),
    ToggleUnique(NodeId),
}

fn matches_filter(p: &OpenProject, id: NodeId) -> bool {
    if p.scene_filter.is_empty() {
        return true;
    }
    let f = p.scene_filter.to_lowercase();
    p.scene.tree.preorder(id).into_iter().any(|n| {
        let node = p.scene.tree.get(n).expect("listed");
        node.name.to_lowercase().contains(&f) || node.get_class().to_lowercase().contains(&f)
    })
}

fn row(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId, depth: usize, actions: &mut Vec<Action>) {
    if !matches_filter(p, id) {
        return;
    }
    let Some(node) = p.scene.tree.get(id) else { return };
    let path = p.scene.tree.path_of(id);
    let has_children = !node.children().is_empty();
    let expanded = p.scene.expanded.contains(&path) || !p.scene_filter.is_empty();
    let selected = p.scene.is_selected(id);
    let info = node.kind.info();
    let color = theme::category_color(info.category);
    let rename_id = egui::Id::unique("rename_node");
    let renaming = ui.ctx().data(|d| d.get_temp::<u64>(rename_id)) == Some(id.to_bits());

    let row_h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let hovered = resp.hovered();
    if selected {
        painter.rect_filled(rect, 6, theme::ACCENT.gamma_multiply(0.35));
        painter.rect_stroke(rect, 6, egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    } else if hovered {
        painter.rect_filled(rect, 6, theme::SURFACE);
    }
    // Drop target highlight.
    if let Some(_payload) = resp.dnd_hover_payload::<DragNode>() {
        painter.rect_stroke(rect, 6, egui::Stroke::new(2.0, theme::ACCENT_2), egui::StrokeKind::Inside);
    }
    let x0 = rect.left() + 6.0 + depth as f32 * 16.0;
    let cy = rect.center().y;
    // Expander arrow.
    let arrow_rect = egui::Rect::from_center_size(egui::pos2(x0 + 6.0, cy), egui::vec2(14.0, 14.0));
    if has_children {
        painter.text(arrow_rect.center(), egui::Align2::CENTER_CENTER, if expanded { "⏷" } else { "⏵" }, egui::FontId::proportional(11.0), theme::TEXT_DIM);
    }
    painter.text(egui::pos2(x0 + 22.0, cy), egui::Align2::CENTER_CENTER, info.icon, egui::FontId::proportional(14.0), color);
    let is_root = id == p.scene.root();
    if !renaming {
        let mut name = RichText::new(&node.name).color(if node.visible { theme::TEXT } else { theme::TEXT_DIM });
        if is_root {
            name = name.strong();
        }
        let galley = ui.painter().layout_no_wrap(name.text().to_owned(), egui::FontId::proportional(13.5), if node.visible { theme::TEXT } else { theme::TEXT_DIM });
        painter.galley(egui::pos2(x0 + 34.0, cy - galley.size().y / 2.0), galley, Color32::WHITE);
    }
    // Badges on the right: unique, script, visibility.
    let mut bx = rect.right() - 12.0;
    let eye_rect = egui::Rect::from_center_size(egui::pos2(bx, cy), egui::vec2(18.0, 18.0));
    if node.kind.is_2d() {
        painter.text(eye_rect.center(), egui::Align2::CENTER_CENTER, if node.visible { "👁" } else { "○" }, egui::FontId::proportional(12.0), if node.visible { theme::TEXT_DIM } else { theme::BORDER });
        bx -= 20.0;
    }
    if node.script.is_some() {
        painter.text(egui::pos2(bx, cy), egui::Align2::CENTER_CENTER, "🔷", egui::FontId::proportional(12.0), theme::ACCENT_2);
        bx -= 20.0;
    }
    if node.unique_name_in_owner {
        painter.text(egui::pos2(bx, cy), egui::Align2::CENTER_CENTER, "%", egui::FontId::monospace(12.0), theme::WARN);
    }
    let conns = p.scene.tree.all_connections(id).len();
    if conns > 0 {
        bx -= 18.0;
        painter.text(egui::pos2(bx, cy), egui::Align2::CENTER_CENTER, "📶", egui::FontId::proportional(11.0), theme::WARN);
    }

    if renaming {
        let edit_rect = egui::Rect::from_min_max(egui::pos2(x0 + 32.0, rect.top() + 2.0), egui::pos2(rect.right() - 40.0, rect.bottom() - 2.0));
        let buf_id = egui::Id::unique("rename_buf");
        let mut buf: String = ui.ctx().data_mut(|d| d.get_temp(buf_id).unwrap_or_else(|| node.name.clone()));
        let r = ui.put(edit_rect, egui::TextEdit::singleline(&mut buf));
        r.request_focus();
        let done = r.lost_focus();
        if done {
            if ui.input(|i| !i.key_pressed(egui::Key::Escape)) && !buf.trim().is_empty() {
                actions.push(Action::Rename(id, buf.trim().to_owned()));
            }
            ui.ctx().data_mut(|d| {
                d.remove::<u64>(rename_id);
                d.remove::<String>(buf_id);
            });
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(buf_id, buf));
        }
    }

    // Interaction.
    if resp.clicked() {
        let pos = resp.interact_pointer_pos().unwrap_or_default();
        if has_children && arrow_rect.expand(4.0).contains(pos) {
            actions.push(Action::Toggle(id));
        } else if node.kind.is_2d() && eye_rect.expand(2.0).contains(pos) {
            actions.push(Action::ToggleVisible(id));
        } else {
            actions.push(Action::Select(id, ui.input(|i| i.modifiers.command || i.modifiers.shift)));
        }
    }
    if resp.double_clicked() {
        if let Some(s) = &node.script {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("open_bp"), s.clone()));
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(rename_id, id.to_bits()));
        }
    }
    if !is_root {
        resp.dnd_set_drag_payload(DragNode(id));
    }
    if let Some(payload) = resp.dnd_release_payload::<DragNode>()
        && payload.0 != id
    {
        actions.push(Action::Reparent(payload.0, id));
    }
    resp.on_hover_text(format!("{} · {}", info.type_name, info.description)).context_menu(|ui| {
        ui.label(RichText::new(format!("{} {}", info.icon, info.type_name)).color(color));
        ui.separator();
        if ui.button("➕  Add child node…").clicked() {
            actions.push(Action::AddChild(id));
            ui.close();
        }
        if ui.button("✏  Rename          F2").clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(rename_id, id.to_bits()));
            ui.close();
        }
        if !is_root {
            if ui.button("🗗  Duplicate       Ctrl+D").clicked() {
                actions.push(Action::Duplicate(id));
                ui.close();
            }
            if ui.button("⬆  Move up").clicked() {
                actions.push(Action::MoveUp(id));
                ui.close();
            }
            if ui.button("⬇  Move down").clicked() {
                actions.push(Action::MoveDown(id));
                ui.close();
            }
            if ui.button("👑  Make scene root").clicked() {
                actions.push(Action::MakeRoot(id));
                ui.close();
            }
        }
        if ui.button(if node.unique_name_in_owner { "%  Revoke unique name" } else { "%  Access as unique name" }).clicked() {
            actions.push(Action::ToggleUnique(id));
            ui.close();
        }
        ui.separator();
        if ui.button("🔷  Attach new blueprint").clicked() {
            actions.push(Action::AttachBlueprint(id));
            ui.close();
        }
        if ui.button("💾  Save branch as scene").clicked() {
            actions.push(Action::SaveBranch(id));
            ui.close();
        }
        if !is_root {
            ui.separator();
            if ui.button(RichText::new("🗑  Delete          Del").color(theme::ERR)).clicked() {
                actions.push(Action::Delete(id));
                ui.close();
            }
        }
    });

    if has_children && expanded {
        for c in p.scene.tree.get_children(id).to_vec() {
            row(ui, p, c, depth + 1, actions);
        }
    }
}

fn apply(p: &mut OpenProject, shared: &mut Shared, a: Action) {
    let doc = &mut p.scene;
    match a {
        Action::Select(id, additive) => doc.select(id, additive),
        Action::Toggle(id) => {
            let path = doc.tree.path_of(id);
            if !doc.expanded.remove(&path) {
                doc.expanded.insert(path);
            }
        }
        Action::Reparent(node, parent) => {
            if node == parent || doc.tree.is_ancestor_of(node, parent) {
                shared.toast(LogLevel::Warn, "Can't move a node into its own subtree");
            } else if let Err(e) = doc.tree.reparent(node, parent, true) {
                shared.toast(LogLevel::Warn, e.to_string());
            } else {
                doc.expanded.insert(doc.tree.path_of(parent));
                doc.touched = true;
            }
        }
        Action::MoveUp(id) | Action::MoveDown(id) => {
            if let Some(parent) = doc.tree.get_parent(id) {
                let i = doc.tree.get_index(id);
                let to = if matches!(a, Action::MoveUp(_)) { (i - 1).max(0) } else { i + 1 };
                if to < doc.tree.get_child_count(parent) as i32 {
                    doc.tree.move_child(parent, id, to);
                    doc.touched = true;
                }
            }
        }
        Action::Delete(id) => {
            if id != doc.root() && doc.tree.contains(id) {
                let _ = doc.tree.free(id);
                doc.selection.retain(|s| doc.tree.contains(*s));
                doc.touched = true;
            }
        }
        Action::Duplicate(id) => {
            if let Some(copy) = doc.tree.duplicate_in_place(id) {
                if let Some(n) = doc.tree.get_mut(copy) {
                    n.transform.position += vd_core::vec2(16.0, 16.0);
                }
                doc.select(copy, false);
                doc.touched = true;
            }
        }
        Action::ToggleVisible(id) => {
            if let Some(n) = doc.tree.get_mut(id) {
                n.visible = !n.visible;
                doc.touched = true;
            }
        }
        Action::Rename(id, name) => {
            doc.tree.set_name(id, &name);
            doc.touched = true;
        }
        Action::AddChild(id) => {
            doc.select(id, false);
            p.dialogs.add_node = Some(AddNodeDialog::default());
        }
        Action::AttachBlueprint(id) => {
            let name = doc.tree.get(id).map(|n| n.name.to_lowercase()).unwrap_or_default();
            let mut path = format!("blueprints/{}.vbp", crate::export::slug(&name));
            let mut i = 2;
            while p.files.contains(&path) {
                path = format!("blueprints/{}_{i}.vbp", crate::export::slug(&name));
                i += 1;
            }
            shared.request(Request::NewBlueprint { path, attach_to: Some(id) });
        }
        Action::SaveBranch(id) => {
            let Some(data) = doc.tree.pack(id) else { return };
            let name = crate::export::slug(&data.name);
            let path = format!("scenes/{name}.vscn");
            let file = SceneFile { format: vd_scene::packed::SCENE_FORMAT_VERSION, root: data };
            match file.save(&p.dir.join(&path)) {
                Ok(()) => {
                    shared.toast(LogLevel::Info, format!("Saved branch as {path}"));
                    shared.request(Request::RefreshFiles);
                }
                Err(e) => shared.toast(LogLevel::Error, e.to_string()),
            }
        }
        Action::MakeRoot(id) => {
            let old = doc.root();
            let Some(old_data) = doc.tree.pack(old) else { return };
            let Some(mut new_data) = doc.tree.pack(id) else { return };
            // New root keeps its own children plus every other old-root child.
            let _ = old_data;
            let others: Vec<_> = doc
                .tree
                .get_children(old)
                .iter()
                .filter(|&&c| c != id && !doc.tree.is_ancestor_of(c, id))
                .filter_map(|&c| doc.tree.pack(c))
                .collect();
            new_data.children.extend(others);
            doc.tree = vd_scene::SceneTree::from_data(&new_data);
            doc.selection.clear();
            doc.touched = true;
        }
        Action::ToggleUnique(id) => {
            if let Some(n) = doc.tree.get_mut(id) {
                n.unique_name_in_owner = !n.unique_name_in_owner;
                doc.touched = true;
            }
        }
    }
}

pub fn add_node_dialog(ctx: &egui::Context, p: &mut OpenProject, shared: &mut Shared) {
    // Double-click on a scripted node opens its blueprint.
    if let Some(path) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::unique("open_bp"))) {
        shared.request(Request::OpenBlueprint(path));
    }
    let Some(dlg) = &mut p.dialogs.add_node else { return };
    let mut open = true;
    let mut chosen: Option<NodeKind> = None;
    egui::Window::new("➕  Add node")
        .open(&mut open)
        .collapsible(false)
        .default_size([460.0, 520.0])
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut dlg.search).hint_text("🔍 Search node types").desired_width(f32::INFINITY));
            if !dlg.focus_set {
                r.request_focus();
                dlg.focus_set = true;
            }
            ui.add_space(6.0);
            let q = dlg.search.to_lowercase();
            let catalog = NodeKind::catalog();
            let filtered: Vec<_> = catalog
                .into_iter()
                .filter(|(info, _)| q.is_empty() || info.type_name.to_lowercase().contains(&q) || info.description.to_lowercase().contains(&q))
                .collect();
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some((_, k)) = filtered.first()
            {
                chosen = Some(k.clone());
            }
            egui::ScrollArea::vertical().auto_shrink(false).max_height(420.0).show(ui, |ui| {
                let mut last_cat = "";
                for (info, kind) in &filtered {
                    if info.category != last_cat {
                        last_cat = info.category;
                        ui.add_space(4.0);
                        ui.label(RichText::new(info.category.to_uppercase()).small().strong().color(theme::TEXT_DIM));
                    }
                    let color = theme::category_color(info.category);
                    let resp = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 5)).corner_radius(6).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(info.icon).size(16.0).color(color));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(info.type_name).strong());
                                ui.label(RichText::new(info.description).small().color(theme::TEXT_DIM));
                            });
                        });
                    });
                    let resp = resp.response.interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(resp.rect, 6, theme::ACCENT.gamma_multiply(0.15));
                    }
                    if resp.clicked() {
                        chosen = Some(kind.clone());
                    }
                }
            });
        });
    if let Some(kind) = chosen {
        let doc = &mut p.scene;
        let parent = doc.primary().unwrap_or(doc.root());
        let name = kind.type_name().to_owned();
        let mut node = Node::new(name, kind);
        // New 2D nodes appear where the editor camera looks.
        let parent_global = doc.tree.compute_global(parent);
        if node.kind.is_2d() && parent == doc.root() {
            node.transform.position = parent_global.inverse().transform_point2(p.cam.center);
        }
        match doc.tree.add_new_child(parent, node) {
            Ok(id) => {
                doc.select(id, false);
                doc.touched = true;
            }
            Err(e) => shared.toast(LogLevel::Error, e.to_string()),
        }
        p.dialogs.add_node = None;
    } else if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        p.dialogs.add_node = None;
    }
}
