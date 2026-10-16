//! Inspector: reflected properties, script, groups, signals and variables.

use eframe::egui::{self, Color32, RichText};
use vd_core::{Color, VType, Variant, Vec2};
use vd_egui::theme;
use vd_engine::LogLevel;
use vd_scene::{NODE_SIGNALS, NodeId, NodeKind, PropHint, PropertyInfo, connect_flags};

use crate::app::{OpenProject, Request, Shared};

/// sRGB-encoded 8-bit color for display of a linear engine color.
pub fn to_color32(c: Color) -> Color32 {
    let [r, g, b, a] = c.to_srgb_u8();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Edits a linear HDR color: a picker for the hue plus an intensity
/// multiplier (values > 1 glow). Returns true if changed.
pub fn hdr_color_edit(ui: &mut egui::Ui, c: &mut Color, hdr: bool) -> bool {
    let intensity = c.r.max(c.g).max(c.b).max(1.0);
    let base = Color::rgba(c.r / intensity, c.g / intensity, c.b / intensity, c.a);
    // egui's picker works in sRGB-encoded floats.
    let [r, g, b, a] = base.to_srgb_u8();
    let mut rgba = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0];
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
            changed = true;
        }
        let mut k = intensity;
        if hdr {
            if ui.add(egui::DragValue::new(&mut k).speed(0.02).range(1.0..=32.0).prefix("×")).on_hover_text("HDR intensity (glow)").changed() {
                changed = true;
            }
        }
        ui.label(RichText::new(base.to_hex()).small().monospace().color(theme::TEXT_DIM));
        if changed {
            let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            let picked = Color::from_srgb_u8(q(rgba[0]), q(rgba[1]), q(rgba[2]), q(rgba[3]));
            *c = Color::rgba(picked.r * k, picked.g * k, picked.b * k, picked.a);
        }
    });
    changed
}

/// Widget for one reflected property. Returns the new value if edited.
pub fn property_widget(ui: &mut egui::Ui, info: &PropertyInfo, value: &Variant, files: &[String]) -> Option<Variant> {
    let mut out = None;
    match (info.ty, info.hint) {
        (VType::Number, PropHint::Angle) => {
            let mut deg = value.to_f32().to_degrees();
            if ui.add(egui::DragValue::new(&mut deg).speed(0.5).suffix("°")).changed() {
                out = Some(Variant::Number(f64::from(deg.to_radians())));
            }
        }
        (VType::Number, PropHint::Range { min, max, step }) => {
            let mut v = value.to_number();
            let speed = if step >= 1.0 { 0.25 } else { f64::from((max - min) / 400.0).max(f64::from(step)) };
            let mut dv = egui::DragValue::new(&mut v).range(f64::from(min)..=f64::from(max)).speed(speed);
            if step >= 1.0 {
                dv = dv.fixed_decimals(0);
            }
            if ui.add(dv).changed() {
                out = Some(Variant::Number(v));
            }
        }
        (VType::Number, _) => {
            let mut v = value.to_number();
            if ui.add(egui::DragValue::new(&mut v).speed(0.5)).changed() {
                out = Some(Variant::Number(v));
            }
        }
        (VType::Bool, _) => {
            let mut b = value.to_bool();
            if ui.checkbox(&mut b, "").changed() {
                out = Some(Variant::Bool(b));
            }
        }
        (VType::Vec2, _) => {
            let mut v = value.to_vec2();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("x").small().color(theme::ERR));
                changed |= ui.add(egui::DragValue::new(&mut v.x).speed(0.5)).changed();
                ui.label(RichText::new("y").small().color(theme::OK));
                changed |= ui.add(egui::DragValue::new(&mut v.y).speed(0.5)).changed();
            });
            if changed {
                out = Some(Variant::Vec2(v));
            }
        }
        (VType::Color, hint) => {
            let mut c = value.to_color();
            if hdr_color_edit(ui, &mut c, hint == PropHint::HdrColor) {
                out = Some(Variant::Color(c));
            }
        }
        (VType::String, PropHint::Enum(names)) => {
            let current = value.to_string();
            egui::ComboBox::from_id_salt(info.name).selected_text(&current).show_ui(ui, |ui| {
                for n in names {
                    if ui.selectable_label(current == *n, *n).clicked() {
                        out = Some(Variant::str(n));
                    }
                }
            });
        }
        (VType::String, PropHint::File(exts)) => {
            let current = value.to_string();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(info.name)
                    .selected_text(if current.is_empty() { "<none>".to_owned() } else { current.clone() })
                    .width(ui.available_width() - 4.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(current.is_empty(), "<none>").clicked() {
                            out = Some(Variant::str(""));
                        }
                        for f in files.iter().filter(|f| exts.iter().any(|e| f.ends_with(&format!(".{e}")))) {
                            if ui.selectable_label(current == *f, f).clicked() {
                                out = Some(Variant::str(f));
                            }
                        }
                    });
            });
        }
        (VType::String, PropHint::Multiline) => {
            let mut s = value.to_string();
            if ui.add(egui::TextEdit::multiline(&mut s).desired_rows(2).desired_width(f32::INFINITY)).changed() {
                out = Some(Variant::str(s));
            }
        }
        _ => {
            let mut s = value.to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(f32::INFINITY)).changed() {
                out = Some(Variant::str(s));
            }
        }
    }
    out
}

fn pretty(name: &str) -> String {
    let mut s = String::new();
    for (i, w) in name.split('_').enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let mut c = w.chars();
        if let Some(f) = c.next() {
            s.extend(f.to_uppercase());
            s.push_str(c.as_str());
        }
    }
    s
}

/// Signals a node of this kind can emit.
pub fn signals_for(kind: &NodeKind) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    match kind {
        NodeKind::Timer(_) => v.push("timeout"),
        NodeKind::Body2D(_) => v.extend(["body_entered", "body_exited"]),
        _ => {}
    }
    v.extend(NODE_SIGNALS);
    v
}

pub fn show(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    theme::section(ui, "🔧", "Inspector");
    ui.add_space(4.0);
    let Some(id) = p.scene.primary() else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Select a node to edit its properties").color(theme::TEXT_DIM));
            ui.add_space(8.0);
            ui.label(RichText::new("Tip: drag nodes in the Scene dock to reparent them,\nright-click for more actions.").small().color(theme::TEXT_DIM));
        });
        return;
    };
    let files = p.files.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        header(ui, p, id);
        ui.add_space(6.0);
        properties(ui, p, id, &files);
        script_section(ui, p, id, shared, &files);
        groups_section(ui, p, id);
        signals_section(ui, p, id, shared);
        variables_section(ui, p, id);
    });
}

fn header(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId) {
    let Some(node) = p.scene.tree.get(id) else { return };
    let info = node.kind.info();
    let color = theme::category_color(info.category);
    let old_name = node.name.clone();
    let mut name = old_name.clone();
    egui::Frame::NONE.fill(theme::SURFACE).corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(info.icon).size(22.0).color(color));
            ui.vertical(|ui| {
                if ui.add(egui::TextEdit::singleline(&mut name).font(egui::TextStyle::Heading).desired_width(ui.available_width())).lost_focus() {
                    if !name.trim().is_empty() && name != old_name {
                        p.scene.tree.set_name(id, name.trim());
                        p.scene.touched = true;
                    }
                }
                ui.label(RichText::new(format!("{}  ·  {}", info.type_name, p.scene.tree.path_of(id))).small().color(theme::TEXT_DIM));
            });
        });
    });
}

fn properties(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId, files: &[String]) {
    let Some(node) = p.scene.tree.get(id) else { return };
    let props = node.property_list();
    let mut groups: Vec<&'static str> = Vec::new();
    for pr in &props {
        if !groups.contains(&pr.group) {
            groups.push(pr.group);
        }
    }
    let mut edits: Vec<(&'static str, Variant)> = Vec::new();
    for g in groups {
        egui::CollapsingHeader::new(RichText::new(g).strong()).default_open(g != "Process" && g != "Node").id_salt(("insp", g)).show(ui, |ui| {
            egui::Grid::new(("grid", g)).num_columns(2).spacing([10.0, 6.0]).min_col_width(96.0).striped(false).show(ui, |ui| {
                for pr in props.iter().filter(|pr| pr.group == g) {
                    let value = node.get(pr.name).unwrap_or_default();
                    ui.label(RichText::new(pretty(pr.name)).color(theme::TEXT_DIM)).on_hover_text(pr.name);
                    if let Some(v) = property_widget(ui, pr, &value, files) {
                        edits.push((pr.name, v));
                    }
                    ui.end_row();
                }
            });
        });
    }
    // Handy actions for specific types.
    if let NodeKind::Sprite2D(s) = &node.kind
        && !s.texture.is_empty()
        && ui.button("📐 Fit size to texture").clicked()
        && let Ok((w, h)) = image::image_dimensions(p.dir.join(&s.texture))
    {
        let frames = Vec2::new(s.hframes.max(1) as f32, s.vframes.max(1) as f32);
        edits.push(("size", Variant::Vec2(Vec2::new(w as f32, h as f32) / frames)));
    }
    if !edits.is_empty() {
        if let Some(n) = p.scene.tree.get_mut(id) {
            for (k, v) in edits {
                n.set(k, &v);
            }
        }
        p.scene.touched = true;
    }
}

fn script_section(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId, shared: &mut Shared, files: &[String]) {
    egui::CollapsingHeader::new(RichText::new("🔷 Script").strong()).default_open(true).show(ui, |ui| {
        let current = p.scene.tree.get(id).and_then(|n| n.script.clone());
        let mut new_script = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("script_pick")
                .selected_text(current.clone().unwrap_or_else(|| "<none>".into()))
                .width(ui.available_width() - 70.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), "<none>").clicked() {
                        new_script = Some(None);
                    }
                    for f in files.iter().filter(|f| f.ends_with(".vbp")) {
                        if ui.selectable_label(current.as_deref() == Some(f), format!("🔷 {f}")).clicked() {
                            new_script = Some(Some(f.clone()));
                        }
                    }
                });
            if let Some(s) = &current {
                if ui.button("Edit").clicked() {
                    shared.request(Request::OpenBlueprint(s.clone()));
                }
            } else if ui.button("New").on_hover_text("Create and attach a new blueprint").clicked() {
                let name = p.scene.tree.get(id).map(|n| crate::export::slug(&n.name)).unwrap_or_default();
                let mut path = format!("blueprints/{name}.vbp");
                let mut i = 2;
                while files.contains(&path) {
                    path = format!("blueprints/{name}_{i}.vbp");
                    i += 1;
                }
                shared.request(Request::NewBlueprint { path, attach_to: Some(id) });
            }
        });
        if let Some(s) = new_script {
            if let Some(n) = p.scene.tree.get_mut(id) {
                n.script = s;
            }
            p.scene.touched = true;
        }
        ui.label(RichText::new("Rust scripts: set the script to \"rust:Name\" and register Name in code.").small().color(theme::TEXT_DIM));
    });
}

fn groups_section(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId) {
    egui::CollapsingHeader::new(RichText::new("🏷 Groups").strong()).default_open(false).show(ui, |ui| {
        let groups: Vec<String> = p.scene.tree.get(id).map(|n| n.groups.iter().map(ToString::to_string).collect()).unwrap_or_default();
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for g in &groups {
                egui::Frame::NONE.fill(theme::SURFACE_HI).corner_radius(10).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(g).small());
                        if ui.small_button("✖").clicked() {
                            remove = Some(g.clone());
                        }
                    });
                });
            }
        });
        let buf_id = ui.scope_id().with("new_group");
        let mut buf: String = ui.data_mut(|d| d.get_temp(buf_id).unwrap_or_default());
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut buf).hint_text("group name").desired_width(140.0));
            if (ui.button("Add").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !buf.trim().is_empty() {
                if let Some(n) = p.scene.tree.get_mut(id) {
                    n.add_to_group(buf.trim());
                }
                p.scene.touched = true;
                buf.clear();
            }
        });
        ui.data_mut(|d| d.insert_temp(buf_id, buf));
        if let Some(g) = remove {
            if let Some(n) = p.scene.tree.get_mut(id) {
                n.remove_from_group(&g);
            }
            p.scene.touched = true;
        }
    });
}

#[derive(Clone, Default)]
struct ConnectForm {
    signal: String,
    target: String,
    method: String,
    one_shot: bool,
}

fn signals_section(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId, shared: &mut Shared) {
    egui::CollapsingHeader::new(RichText::new("📶 Signals").strong()).default_open(false).show(ui, |ui| {
        let Some(node) = p.scene.tree.get(id) else { return };
        let signals = signals_for(&node.kind);
        let node_name = node.name.clone();
        let scene_root = p.scene.root();
        let conns = p.scene.tree.all_connections(id).to_vec();
        if conns.is_empty() {
            ui.label(RichText::new("No connections").small().color(theme::TEXT_DIM));
        }
        let mut remove = None;
        for c in &conns {
            ui.horizontal(|ui| {
                ui.label(RichText::new(c.signal.as_str()).monospace().color(theme::WARN));
                ui.label("»");
                let target = p.scene.tree.get_path_to(scene_root, c.target);
                ui.label(RichText::new(format!("{target}::{}", c.method)).monospace());
                if ui.small_button("🗑").clicked() {
                    remove = Some(c.clone());
                }
            });
        }
        if let Some(c) = remove {
            let _ = p.scene.tree.disconnect(id, c.signal.as_str(), c.target, c.method.as_str());
            p.scene.touched = true;
        }
        ui.separator();
        let form_id = ui.scope_id().with(("connect", id.to_bits()));
        let mut form: ConnectForm = ui.data_mut(|d| d.get_temp(form_id).unwrap_or_default());
        if form.signal.is_empty() {
            form.signal = signals[0].to_owned();
        }
        if form.target.is_empty() {
            form.target = ".".into();
        }
        egui::Grid::new("connect_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Signal");
            egui::ComboBox::from_id_salt("sig").selected_text(&form.signal).show_ui(ui, |ui| {
                for s in &signals {
                    ui.selectable_value(&mut form.signal, (*s).to_owned(), *s);
                }
            });
            ui.end_row();
            ui.label("Target");
            egui::ComboBox::from_id_salt("tgt").selected_text(&form.target).show_ui(ui, |ui| {
                for n in p.scene.tree.preorder(scene_root) {
                    let rel = p.scene.tree.get_path_to(scene_root, n);
                    let name = p.scene.tree.get(n).map(|x| x.name.clone()).unwrap_or_default();
                    ui.selectable_value(&mut form.target, rel.clone(), format!("{name}  ({rel})"));
                }
            });
            ui.end_row();
            ui.label("Method");
            let default_method = format!("_on_{}_{}", crate::export::slug(&node_name).replace('-', "_"), form.signal);
            ui.add(egui::TextEdit::singleline(&mut form.method).hint_text(&default_method));
            ui.end_row();
            ui.label("One shot");
            ui.checkbox(&mut form.one_shot, "");
            ui.end_row();
        });
        if theme::accent_button(ui, "Connect", theme::ACCENT).clicked() {
            let method = if form.method.trim().is_empty() {
                format!("_on_{}_{}", crate::export::slug(&node_name).replace('-', "_"), form.signal)
            } else {
                form.method.trim().to_owned()
            };
            match p.scene.tree.get_node_or_null(scene_root, &form.target) {
                Some(target) => {
                    let flags = connect_flags::PERSIST | if form.one_shot { connect_flags::ONE_SHOT } else { 0 };
                    match p.scene.tree.connect(id, &form.signal, target, &method, flags) {
                        Ok(()) => {
                            p.scene.touched = true;
                            // Like Godot creating a method stub: add the handler event.
                            if let Some(bp) = p.scene.tree.get(target).and_then(|n| n.script.clone()).filter(|s| s.ends_with(".vbp")) {
                                shared.request(Request::AddMethodHandler { blueprint: bp, method: method.clone() });
                            } else {
                                shared.toast(LogLevel::Info, format!("Connected. Attach a blueprint to the target and add an \"On Event / Method\" named {method}."));
                            }
                            form.method.clear();
                        }
                        Err(e) => shared.toast(LogLevel::Warn, e.to_string()),
                    }
                }
                None => shared.toast(LogLevel::Warn, "Target node not found"),
            }
        }
        ui.data_mut(|d| d.insert_temp(form_id, form));
    });
}

fn variables_section(ui: &mut egui::Ui, p: &mut OpenProject, id: NodeId) {
    egui::CollapsingHeader::new(RichText::new("📦 Variables").strong()).default_open(false).show(ui, |ui| {
        // Exposed blueprint variables can be overridden per node.
        let exposed: Vec<vd_blueprint::VarDef> = p
            .scene
            .tree
            .get(id)
            .and_then(|n| n.script.clone())
            .and_then(|s| p.blueprints.iter().find(|b| b.path == s).map(|b| b.bp.variables.clone()).or_else(|| vd_blueprint::Blueprint::load(&p.dir.join(&s)).ok().map(|b| b.variables)))
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v.exposed)
            .collect();
        let Some(node) = p.scene.tree.get_mut(id) else { return };
        let mut changed = false;
        if !exposed.is_empty() {
            ui.label(RichText::new("Blueprint (exposed)").small().color(theme::TEXT_DIM));
            for v in &exposed {
                ui.horizontal(|ui| {
                    let cur = node.var(&v.name).cloned();
                    let overridden = cur.is_some();
                    let value = cur.unwrap_or_else(|| v.default.clone());
                    ui.label(RichText::new(&v.name).color(if overridden { theme::ACCENT_2 } else { theme::TEXT_DIM }));
                    let info = PropertyInfo { name: "var", ty: v.ty, hint: PropHint::None, group: "" };
                    if let Some(nv) = property_widget(ui, &info, &value, &[]) {
                        node.set(&v.name, &nv);
                        changed = true;
                    }
                    if overridden && ui.small_button("↺").on_hover_text("Reset to blueprint default").clicked() {
                        node.vars.retain(|(n, _)| n != &v.name);
                        changed = true;
                    }
                });
            }
            ui.separator();
        }
        ui.label(RichText::new("Custom node variables (read with Get Property)").small().color(theme::TEXT_DIM));
        let mut remove = None;
        for (i, (name, value)) in node.vars.iter_mut().enumerate() {
            if exposed.iter().any(|v| &v.name == name) {
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(name.as_str()).monospace());
                let info = PropertyInfo { name: "var", ty: value.vtype(), hint: PropHint::None, group: "" };
                if let Some(nv) = property_widget(ui, &info, value, &[]) {
                    *value = nv;
                    changed = true;
                }
                if ui.small_button("🗑").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            node.vars.remove(i);
            changed = true;
        }
        let buf_id = ui.scope_id().with("new_var");
        let (mut name, mut ty): (String, usize) = ui.data_mut(|d| d.get_temp(buf_id).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("name").desired_width(100.0));
            let types = [VType::Number, VType::Bool, VType::String, VType::Vec2, VType::Color];
            egui::ComboBox::from_id_salt("var_ty").selected_text(types[ty.min(4)].name()).width(80.0).show_ui(ui, |ui| {
                for (i, t) in types.iter().enumerate() {
                    ui.selectable_value(&mut ty, i, t.name());
                }
            });
            if ui.button("Add").clicked() && !name.trim().is_empty() {
                node.vars.push((name.trim().to_owned(), types[ty.min(4)].default_value()));
                name.clear();
                changed = true;
            }
        });
        ui.data_mut(|d| d.insert_temp(buf_id, (name, ty)));
        if changed {
            p.scene.touched = true;
        }
    });
}
