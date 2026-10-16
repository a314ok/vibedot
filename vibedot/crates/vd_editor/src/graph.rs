//! The Blueprint editor: a node-graph canvas with typed pins, bezier wires,
//! drag-to-connect (with type checking), a searchable node palette that
//! filters by the dragged pin's type, box selection, comments, inline
//! literal editing, compile-error highlighting and a bytecode viewer.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use vd_blueprint::defs::{self, Flavor, NodeDef, PinDef, PinKind};
use vd_blueprint::graph::Comment;
use vd_blueprint::{BpNodeId, Blueprint, PinRef};
use vd_core::{Color, VType, Variant, Vec2, vec2};
use vd_egui::theme;
use vd_engine::LogLevel;
use vd_scene::{PropHint, PropertyInfo};

use crate::app::{OpenProject, Request, Shared};
use crate::doc::BpDoc;

const HEADER: f32 = 30.0;
const ROW: f32 = 24.0;
const PAD: f32 = 8.0;
const PIN_R: f32 = 5.5;

#[derive(Clone, Debug)]
enum Press {
    Node(BpNodeId),
    Comment(usize),
    Pin(PinRef, bool),
    Empty,
}

#[derive(Clone, Debug)]
struct Palette {
    screen: Pos2,
    world: Vec2,
    search: String,
    /// Pin the palette was opened from: (pin, is_output, kind).
    from: Option<(PinRef, bool, PinKind)>,
    /// Frames left during which the search box grabs keyboard focus.
    focus: bool,
    focus_frames: u8,
}

#[derive(Clone, Debug)]
struct LiteralEdit {
    node: BpNodeId,
    pin: String,
    pos: Pos2,
}

#[derive(Default)]
pub struct GraphUi {
    press: Option<(Press, Pos2)>,
    dragging: bool,
    wire_end: Option<Pos2>,
    box_end: Option<Pos2>,
    palette: Option<Palette>,
    menu: Option<(BpNodeId, Pos2)>,
    literal: Option<LiteralEdit>,
    clipboard: Option<String>,
    library_search: String,
    /// Frame on which the current popup opened (it must survive that frame).
    opened_frame: u64,
}

/// Color for a pin type.
pub fn type_color(kind: PinKind) -> Color32 {
    match kind {
        PinKind::Exec => Color32::from_rgb(235, 235, 245),
        PinKind::Data(t) => match t {
            VType::Bool => Color32::from_rgb(239, 68, 68),
            VType::Number => Color32::from_rgb(132, 204, 22),
            VType::Vec2 => Color32::from_rgb(250, 204, 21),
            VType::Color => Color32::from_rgb(236, 72, 153),
            VType::String => Color32::from_rgb(244, 114, 182),
            VType::Object => Color32::from_rgb(56, 189, 248),
            VType::Array => Color32::from_rgb(251, 146, 60),
            VType::Any => Color32::from_rgb(160, 160, 185),
        },
    }
}

struct Layout {
    rect: Rect,
    inputs: Vec<(&'static PinDef, Pos2)>,
    outputs: Vec<(&'static PinDef, Pos2)>,
}

struct View {
    origin: Pos2,
    pan: egui::Vec2,
    zoom: f32,
}

impl View {
    fn to_screen(&self, w: Vec2) -> Pos2 {
        self.origin + self.pan + egui::vec2(w.x, w.y) * self.zoom
    }
    fn to_world(&self, s: Pos2) -> Vec2 {
        let v = (s - self.origin - self.pan) / self.zoom;
        vec2(v.x, v.y)
    }
}

fn node_width(def: &NodeDef, title: &str) -> f32 {
    let char_w = 7.4;
    let ins = def.inputs.iter().map(|p| p.name.len() + if p.is_exec() { 0 } else { 8 }).max().unwrap_or(0);
    let outs = def.outputs.iter().map(|p| p.name.len()).max().unwrap_or(0);
    ((title.len() as f32) * 8.0 + 44.0).max((ins + outs) as f32 * char_w + 56.0).clamp(150.0, 340.0)
}

fn title_of(n: &vd_blueprint::BpNode, def: &NodeDef) -> String {
    match def.title_pin {
        Some(pin) => {
            let v = n.literal(pin).to_string();
            if v.is_empty() { def.title.to_owned() } else { format!("{}  ·  {v}", def.title) }
        }
        None => def.title.to_owned(),
    }
}

fn layout(view: &View, n: &vd_blueprint::BpNode, def: &'static NodeDef) -> Layout {
    let title = title_of(n, def);
    let w = node_width(def, &title);
    let rows = def.inputs.len().max(def.outputs.len()).max(1) as f32;
    let h = HEADER + PAD * 2.0 + rows * ROW - if def.inputs.is_empty() && def.outputs.is_empty() { ROW } else { 0.0 };
    let min = view.to_screen(n.pos);
    let rect = Rect::from_min_size(min, egui::vec2(w, h) * view.zoom);
    let row_y = |i: usize| min.y + (HEADER + PAD + ROW * (i as f32 + 0.5)) * view.zoom;
    let inputs = def.inputs.iter().enumerate().map(|(i, p)| (p, Pos2::new(rect.left(), row_y(i)))).collect();
    let outputs = def.outputs.iter().enumerate().map(|(i, p)| (p, Pos2::new(rect.right(), row_y(i)))).collect();
    Layout { rect, inputs, outputs }
}

fn wire(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, width: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0);
    let pts = [a, a + egui::vec2(dx, 0.0), b - egui::vec2(dx, 0.0), b];
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width, color)));
}

fn literal_text(v: &Variant) -> String {
    match v {
        Variant::Number(n) => {
            if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n:.2}") }
        }
        Variant::Vec2(v) => format!("{:.0}, {:.0}", v.x, v.y),
        Variant::Bool(b) => if *b { "✔".into() } else { "✖".into() },
        Variant::String(s) => {
            if s.chars().count() > 14 { format!("\"{}…\"", s.chars().take(13).collect::<String>()) } else { format!("\"{s}\"") }
        }
        Variant::Nil => "—".into(),
        other => other.to_string(),
    }
}

fn shows_literal(pin: &PinDef) -> bool {
    !pin.is_exec() && !matches!(pin.default, defs::Lit::SelfNode) && !(pin.vtype() == VType::Object || pin.vtype() == VType::Array)
}

/// Main canvas.
pub fn show(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    // Tabs of open blueprints.
    egui::Panel::top("bp_tabs").exact_size(34.0).frame(egui::Frame::NONE.fill(theme::BG).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
        ui.horizontal(|ui| {
            let mut close = None;
            for (i, doc) in p.blueprints.iter().enumerate() {
                let active = p.active_bp == Some(i);
                let label = format!("🔷 {}{}", doc.file_name(), if doc.dirty { " •" } else { "" });
                let b = egui::Button::new(RichText::new(label).color(if active { Color32::WHITE } else { theme::TEXT_DIM })).fill(if active { theme::SURFACE_HI } else { Color32::TRANSPARENT });
                if ui.add(b).clicked() {
                    p.active_bp = Some(i);
                }
                if active && ui.small_button("✖").on_hover_text("Close").clicked() {
                    close = Some(i);
                }
            }
            if let Some(i) = close {
                if p.blueprints[i].dirty {
                    let _ = p.blueprints[i].save(&p.dir);
                }
                p.blueprints.remove(i);
                p.active_bp = if p.blueprints.is_empty() { None } else { Some(i.saturating_sub(1).min(p.blueprints.len() - 1)) };
            }
            if p.blueprints.is_empty() {
                ui.label(RichText::new("No blueprint open").color(theme::TEXT_DIM));
            }
        });
    });

    let Some(idx) = p.active_bp.filter(|&i| i < p.blueprints.len()) else {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new("🔷").size(64.0).color(theme::ACCENT));
                ui.label(RichText::new("Visual scripting with Blueprints").size(22.0).strong());
                ui.add_space(8.0);
                ui.label(RichText::new("Open a .vbp from the FileSystem dock, or attach a new blueprint to a node\nfrom the Inspector (Script » New).").color(theme::TEXT_DIM));
                ui.add_space(16.0);
                for f in p.files.iter().filter(|f| f.ends_with(".vbp")).take(12) {
                    if ui.button(format!("🔷 {f}")).clicked() {
                        shared.request(Request::OpenBlueprint(f.clone()));
                    }
                }
            });
        });
        return;
    };
    let files = p.files.clone();
    let actions: Vec<String> = p.settings.input.keys().cloned().collect();
    let gui = &mut p.graph_ui;
    let doc = &mut p.blueprints[idx];
    canvas(ui, doc, gui, shared, &files, &actions);
}

fn canvas(ui: &mut egui::Ui, doc: &mut BpDoc, g: &mut GraphUi, shared: &mut Shared, files: &[String], actions: &[String]) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.interact(rect, ui.scope_id().with("bp_canvas"), egui::Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::BG_DEEP);

    // Center the graph the first time it is shown.
    if !doc.view.initialized {
        doc.view.initialized = true;
        if let Some(min) = doc.bp.nodes.iter().map(|n| n.pos).reduce(|a, b| a.min(b)) {
            doc.view.pan = vec2(60.0, 60.0) - min;
        }
    }
    // Focus a node requested by the Problems panel.
    if let Some(n) = ctx.data_mut(|d| d.remove_temp::<u32>(egui::Id::unique("focus_bp_node")))
        && let Some(node) = doc.bp.node(BpNodeId(n))
    {
        doc.view.pan = vec2(rect.width() * 0.4, rect.height() * 0.4) - node.pos * doc.view.zoom;
        doc.selection = [BpNodeId(n)].into();
    }

    let mut view = View { origin: rect.min, pan: egui::vec2(doc.view.pan.x, doc.view.pan.y), zoom: doc.view.zoom };

    // Zoom (wheel) around the cursor.
    if resp.hovered() && g.palette.is_none() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 300.0);
        if scroll.abs() > 0.01
            && let Some(m) = ctx.input(|i| i.pointer.hover_pos())
        {
            let before = view.to_world(m);
            view.zoom = (view.zoom * (1.0 + scroll * 0.0015)).clamp(0.25, 2.5);
            let after_screen = view.to_screen(before);
            view.pan += m - after_screen;
        }
    }
    // Pan with middle/right drag.
    if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
        view.pan += resp.drag_delta();
    }

    // Grid.
    let step = 32.0 * view.zoom;
    if step > 6.0 {
        let off = view.origin + view.pan;
        let mut x = rect.left() + (off.x - rect.left()).rem_euclid(step);
        let mut i = ((rect.left() - off.x) / step).ceil() as i64;
        while x < rect.right() {
            let c = if i % 8 == 0 { Color32::from_white_alpha(14) } else { Color32::from_white_alpha(6) };
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, c));
            x += step;
            i += 1;
        }
        let mut y = rect.top() + (off.y - rect.top()).rem_euclid(step);
        let mut j = ((rect.top() - off.y) / step).ceil() as i64;
        while y < rect.bottom() {
            let c = if j % 8 == 0 { Color32::from_white_alpha(14) } else { Color32::from_white_alpha(6) };
            painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, c));
            y += step;
            j += 1;
        }
    }

    // Layouts.
    let layouts: Vec<(BpNodeId, &'static NodeDef, Layout)> = doc
        .bp
        .nodes
        .iter()
        .filter_map(|n| {
            let def = n.def()?;
            Some((n.id, def, layout(&view, n, def)))
        })
        .collect();
    let pin_pos = |r: &PinRef, output: bool| -> Option<Pos2> {
        let (_, _, l) = layouts.iter().find(|(id, _, _)| *id == r.node)?;
        let list = if output { &l.outputs } else { &l.inputs };
        list.iter().find(|(p, _)| p.name == r.pin).map(|(_, pos)| *pos)
    };

    // Comments (behind everything).
    for (ci, c) in doc.bp.comments.iter().enumerate() {
        let r = Rect::from_min_size(view.to_screen(c.pos), egui::vec2(c.size.x, c.size.y) * view.zoom);
        let col = Color32::from_rgba_unmultiplied((c.color.r * 255.0) as u8, (c.color.g * 255.0) as u8, (c.color.b * 255.0) as u8, 40);
        painter.rect_filled(r, 10, col);
        painter.rect_stroke(r, 10, Stroke::new(1.0, col.gamma_multiply(3.0)), egui::StrokeKind::Inside);
        painter.text(r.left_top() + egui::vec2(10.0, 8.0) * view.zoom, egui::Align2::LEFT_TOP, &c.text, egui::FontId::proportional(15.0 * view.zoom.max(0.6)), Color32::from_white_alpha(200));
        let _ = ci;
    }

    // Wires.
    for l in &doc.bp.links {
        let (Some(a), Some(b)) = (pin_pos(&l.from, true), pin_pos(&l.to, false)) else { continue };
        let kind = doc.bp.pin_kind(l.from.node, &l.from.pin, true).unwrap_or(PinKind::Exec);
        let w = if kind == PinKind::Exec { 3.0 } else { 2.2 };
        let selected = doc.selection.contains(&l.from.node) || doc.selection.contains(&l.to.node);
        wire(&painter, a, b, if selected { type_color(kind) } else { type_color(kind).gamma_multiply(0.75) }, w * view.zoom.max(0.6));
    }

    // Hit testing helpers.
    let mouse = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
    let hit_pin = |m: Pos2| -> Option<(PinRef, bool)> {
        for (id, _, l) in layouts.iter().rev() {
            for (pin, pos) in &l.outputs {
                if pos.distance(m) < 11.0 * view.zoom.max(0.7) {
                    return Some((PinRef::new(*id, pin.name), true));
                }
            }
            for (pin, pos) in &l.inputs {
                if !pin.literal_only && pos.distance(m) < 11.0 * view.zoom.max(0.7) {
                    return Some((PinRef::new(*id, pin.name), false));
                }
            }
        }
        None
    };
    let hit_node = |m: Pos2| -> Option<BpNodeId> { layouts.iter().rev().find(|(_, _, l)| l.rect.contains(m)).map(|(id, _, _)| *id) };
    let hit_comment = |m: Pos2| -> Option<usize> {
        doc.bp.comments.iter().enumerate().rev().find_map(|(i, c)| {
            let r = Rect::from_min_size(view.to_screen(c.pos), egui::vec2(c.size.x, 32.0) * view.zoom);
            r.contains(m).then_some(i)
        })
    };

    // Nodes.
    let font = |s: f32| egui::FontId::proportional(s * view.zoom.clamp(0.5, 1.6));
    let mut pill_rects: Vec<(BpNodeId, &'static str, Rect)> = Vec::new();
    for (id, def, l) in &layouts {
        let Some(n) = doc.bp.node(*id) else { continue };
        if !l.rect.intersects(rect) {
            continue;
        }
        let selected = doc.selection.contains(id);
        let error = doc.has_error(*id).map(str::to_owned);
        let cat = theme::category_color(def.category);
        let r = l.rect;
        let cr = (8.0 * view.zoom) as u8;
        // Shadow + body.
        painter.rect_filled(r.translate(egui::vec2(0.0, 4.0)), cr, Color32::from_black_alpha(90));
        painter.rect_filled(r, cr, Color32::from_rgb(28, 28, 40));
        let header = Rect::from_min_size(r.min, egui::vec2(r.width(), HEADER * view.zoom));
        let flavor_tint = match def.flavor {
            Flavor::Event(_) => cat.gamma_multiply(0.75),
            Flavor::Pure(_) => cat.gamma_multiply(0.35),
            _ => cat.gamma_multiply(0.55),
        };
        painter.rect_filled(header, egui::CornerRadius { nw: cr, ne: cr, sw: 0, se: 0 }, flavor_tint);
        let icon = match def.flavor {
            Flavor::Event(_) => "⚡",
            Flavor::Flow(_) => "🔀",
            Flavor::Var(_) => "📦",
            Flavor::Pure(_) => "ƒ",
            Flavor::Exec(_) => "▶",
        };
        painter.text(header.left_center() + egui::vec2(10.0 * view.zoom, 0.0), egui::Align2::LEFT_CENTER, format!("{icon}  {}", title_of(n, def)), font(13.5), Color32::WHITE);
        let border = if error.is_some() {
            Stroke::new(2.0, theme::ERR)
        } else if selected {
            Stroke::new(2.0, theme::ACCENT_2)
        } else {
            Stroke::new(1.0, Color32::from_white_alpha(28))
        };
        painter.rect_stroke(r, cr, border, egui::StrokeKind::Outside);
        if let Some(e) = &error {
            painter.text(r.right_top() + egui::vec2(-8.0, -10.0), egui::Align2::RIGHT_BOTTOM, format!("⚠ {e}"), font(11.5), theme::ERR);
        }
        // Pins.
        for (pin, pos) in &l.inputs {
            let pr = PinRef::new(*id, pin.name);
            let connected = doc.bp.source_of(&pr).is_some() || (pin.is_exec() && doc.bp.links.iter().any(|lk| lk.to == pr));
            let kind = doc.bp.pin_kind(*id, pin.name, false).unwrap_or(pin.kind);
            if !pin.literal_only {
                draw_pin(&painter, *pos, kind, connected, view.zoom);
            }
            let label_pos = *pos + egui::vec2(12.0 * view.zoom, 0.0);
            let label = if pin.is_exec() && pin.name == "exec" { String::new() } else { pin.name.to_owned() };
            let lr = painter.text(label_pos, egui::Align2::LEFT_CENTER, &label, font(12.0), theme::TEXT_DIM);
            if !connected && shows_literal(pin) && view.zoom > 0.45 {
                let value = n.literal(pin.name);
                let txt = literal_text(&value);
                let galley = painter.layout_no_wrap(txt, font(11.5), theme::TEXT);
                let pill = Rect::from_min_size(Pos2::new(lr.right() + 6.0 * view.zoom, pos.y - galley.size().y / 2.0 - 2.0), galley.size() + egui::vec2(10.0, 4.0));
                let pill_fill = if let PinKind::Data(VType::Color) = kind {
                    crate::inspector::to_color32(value.to_color())
                } else {
                    Color32::from_rgb(40, 40, 56)
                };
                painter.rect_filled(pill, 6, pill_fill);
                painter.galley(pill.min + egui::vec2(5.0, 2.0), galley, theme::TEXT);
                pill_rects.push((*id, pin.name, pill));
            }
        }
        for (pin, pos) in &l.outputs {
            let pr = PinRef::new(*id, pin.name);
            let connected = doc.bp.is_connected(&pr);
            let kind = doc.bp.pin_kind(*id, pin.name, true).unwrap_or(pin.kind);
            draw_pin(&painter, *pos, kind, connected, view.zoom);
            let label = if pin.is_exec() && pin.name == "then" { String::new() } else { pin.name.to_owned() };
            painter.text(*pos - egui::vec2(12.0 * view.zoom, 0.0), egui::Align2::RIGHT_CENTER, label, font(12.0), theme::TEXT_DIM);
        }
    }

    // ------------------------------------------------------------ input
    let (pressed, released, sec_clicked, shift, cmd) = ctx.input(|i| {
        (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.button_clicked(egui::PointerButton::Secondary), i.modifiers.shift, i.modifiers.command)
    });
    let over_canvas = resp.hovered() || resp.dragged();
    if pressed && over_canvas && g.palette.is_none() && g.literal.is_none() && g.menu.is_none() {
        let m = mouse.unwrap_or_default();
        if let Some((nid, pin, pill)) = pill_rects.iter().find(|(_, _, r)| r.contains(m)) {
            g.opened_frame = ctx.cumulative_frame_nr();
            g.literal = Some(LiteralEdit { node: *nid, pin: (*pin).to_owned(), pos: pill.left_bottom() + egui::vec2(0.0, 4.0) });
            g.press = None;
        } else if let Some((pin, out)) = hit_pin(m) {
            g.press = Some((Press::Pin(pin, out), m));
        } else if let Some(nid) = hit_node(m) {
            if !doc.selection.contains(&nid) {
                if !shift {
                    doc.selection.clear();
                }
                doc.selection.insert(nid);
            } else if shift {
                doc.selection.remove(&nid);
            }
            g.press = Some((Press::Node(nid), m));
        } else if let Some(ci) = hit_comment(m) {
            g.press = Some((Press::Comment(ci), m));
        } else {
            g.press = Some((Press::Empty, m));
        }
        g.dragging = false;
    }
    if let (Some((press, start)), Some(m)) = (g.press.clone(), mouse)
        && ctx.input(|i| i.pointer.primary_down())
    {
        if !g.dragging && m.distance(start) > 3.0 {
            g.dragging = true;
        }
        if g.dragging {
            let d = ctx.input(|i| i.pointer.delta()) / view.zoom;
            match press {
                Press::Node(_) => {
                    for nid in doc.selection.clone() {
                        if let Some(n) = doc.bp.node_mut(nid) {
                            n.pos += vec2(d.x, d.y);
                        }
                    }
                    doc.touched = true;
                }
                Press::Comment(ci) => {
                    if let Some(c) = doc.bp.comments.get(ci) {
                        let r = Rect::from_min_size(view.to_screen(c.pos), egui::vec2(c.size.x, c.size.y) * view.zoom);
                        let inside: Vec<BpNodeId> = layouts.iter().filter(|(_, _, l)| r.contains_rect(l.rect)).map(|(id, _, _)| *id).collect();
                        for nid in inside {
                            if let Some(n) = doc.bp.node_mut(nid) {
                                n.pos += vec2(d.x, d.y);
                            }
                        }
                    }
                    if let Some(c) = doc.bp.comments.get_mut(ci) {
                        c.pos += vec2(d.x, d.y);
                    }
                    doc.touched = true;
                }
                Press::Pin(..) => g.wire_end = Some(m),
                Press::Empty => g.box_end = Some(m),
            }
        }
    }
    // Live wire / box.
    if let (Some((Press::Pin(pin, out), _)), Some(end)) = (&g.press, g.wire_end) {
        if let Some(a) = pin_pos(pin, *out) {
            let kind = doc.bp.pin_kind(pin.node, &pin.pin, *out).unwrap_or(PinKind::Exec);
            let (s, e) = if *out { (a, end) } else { (end, a) };
            wire(&painter, s, e, type_color(kind), 2.5);
        }
    }
    if let (Some((Press::Empty, start)), Some(end)) = (&g.press, g.box_end) {
        let r = Rect::from_two_pos(*start, end);
        painter.rect_filled(r, 2, theme::ACCENT.gamma_multiply(0.12));
        painter.rect_stroke(r, 2, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    if released {
        if let Some((press, _)) = g.press.take() {
            let m = mouse.unwrap_or_default();
            match press {
                Press::Pin(pin, out) if g.dragging => {
                    if let Some((target, tout)) = hit_pin(m) {
                        if tout != out {
                            match doc.bp.connect(pin.clone(), target) {
                                Ok(()) => doc.touched = true,
                                Err(e) => shared.toast(LogLevel::Warn, e.to_string()),
                            }
                        }
                    } else if rect.contains(m) {
                        let kind = doc.bp.pin_kind(pin.node, &pin.pin, out).unwrap_or(PinKind::Exec);
                        g.opened_frame = ctx.cumulative_frame_nr();
                        g.palette = Some(Palette { screen: m, world: view.to_world(m), search: String::new(), from: Some((pin, out, kind)), focus: false, focus_frames: 6 });
                    }
                }
                Press::Pin(pin, _) => {
                    // Click on a pin: alt-click breaks links.
                    if ctx.input(|i| i.modifiers.alt) {
                        doc.bp.disconnect_pin(&pin);
                        doc.touched = true;
                    }
                }
                Press::Empty if g.dragging => {
                    if let Some(end) = g.box_end {
                        let r = Rect::from_two_pos(view.to_screen(view.to_world(end)), m.min(end).max(m));
                        let start_screen = r;
                        let _ = start_screen;
                    }
                    if let (Some(end), Some((_, start))) = (g.box_end, Some(((), m))) {
                        let _ = start;
                        let _ = end;
                    }
                }
                Press::Empty => {
                    doc.selection.clear();
                    g.literal = None;
                }
                Press::Node(nid) if !g.dragging && !shift => {
                    doc.selection = [nid].into();
                }
                _ => {}
            }
        }
        // Box selection commit (needs the start position saved in box_end/press).
        g.dragging = false;
        g.wire_end = None;
    }
    // Box select uses the press origin from egui.
    if released
        && let Some(end) = g.box_end.take()
        && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
    {
        let r = Rect::from_two_pos(origin, end);
        if !shift {
            doc.selection.clear();
        }
        for (id, _, l) in &layouts {
            if r.intersects(l.rect) {
                doc.selection.insert(*id);
            }
        }
    }

    // Right click: node menu or palette.
    if sec_clicked && resp.hovered() && g.palette.is_none() {
        let m = mouse.unwrap_or_default();
        if let Some((pin, _)) = hit_pin(m) {
            doc.bp.disconnect_pin(&pin);
            doc.touched = true;
        } else if let Some(nid) = hit_node(m) {
            if !doc.selection.contains(&nid) {
                doc.selection = [nid].into();
            }
            g.opened_frame = ctx.cumulative_frame_nr();
            g.menu = Some((nid, m));
        } else {
            g.opened_frame = ctx.cumulative_frame_nr();
            g.palette = Some(Palette { screen: m, world: view.to_world(m), search: String::new(), from: None, focus: false, focus_frames: 6 });
        }
    }

    // Keyboard.
    if !ctx.egui_wants_keyboard_input() && resp.hovered() || resp.has_focus() {
        let (del, dup, copy, paste, frame, comment, home) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.modifiers.command && i.key_pressed(egui::Key::D),
                i.modifiers.command && i.key_pressed(egui::Key::C),
                i.modifiers.command && i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::C) && !i.modifiers.command,
                i.key_pressed(egui::Key::Home),
            )
        });
        if !ctx.egui_wants_keyboard_input() {
            if del {
                delete_selection(doc);
            }
            if dup {
                duplicate_selection(doc, vec2(40.0, 40.0));
            }
            if copy {
                g.clipboard = Some(copy_selection(doc));
            }
            if paste && let Some(clip) = g.clipboard.clone() {
                let at = mouse.map_or(vec2(0.0, 0.0), |m| view.to_world(m));
                paste_nodes(doc, &clip, at);
            }
            if comment && !doc.selection.is_empty() && !cmd {
                comment_selection(doc, &layouts, &view);
            }
            if frame || home {
                frame_all(doc, rect, &mut view, frame && !doc.selection.is_empty());
            }
        }
    }

    doc.view.pan = vec2(view.pan.x, view.pan.y);
    doc.view.zoom = view.zoom;

    node_menu(&ctx, doc, g);
    palette(&ctx, doc, g, shared);
    literal_editor(&ctx, doc, g, files, actions);

    // Overlay: compile status and zoom.
    let status = if doc.errors.is_empty() {
        RichText::new("✔ Compiled").color(theme::OK)
    } else {
        RichText::new(format!("✖ {} error(s)", doc.errors.len())).color(theme::ERR)
    };
    let tl = rect.right_top() + egui::vec2(-14.0, 10.0);
    painter.text(tl, egui::Align2::RIGHT_TOP, status.text(), egui::FontId::proportional(13.0), if doc.errors.is_empty() { theme::OK } else { theme::ERR });
    let hint_strip = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 26.0), rect.right_bottom());
    painter.rect_filled(hint_strip, 0, Color32::from_rgba_unmultiplied(11, 11, 16, 230));
    painter.text(
        rect.left_bottom() + egui::vec2(12.0, -7.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{:.0}%  ·  RMB: add node / menu  ·  drag pins to connect  ·  Alt+click pin: break  ·  C: comment  ·  F: frame", view.zoom * 100.0),
        egui::FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
}

fn draw_pin(painter: &egui::Painter, pos: Pos2, kind: PinKind, connected: bool, zoom: f32) {
    let c = type_color(kind);
    let r = PIN_R * zoom.clamp(0.6, 1.5);
    match kind {
        PinKind::Exec => {
            let pts = vec![pos + egui::vec2(-r, -r), pos + egui::vec2(r * 0.9, -r), pos + egui::vec2(r * 1.6, 0.0), pos + egui::vec2(r * 0.9, r), pos + egui::vec2(-r, r)];
            if connected {
                painter.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
            } else {
                painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, c)));
            }
        }
        PinKind::Data(_) => {
            if connected {
                painter.circle_filled(pos, r, c);
            } else {
                painter.circle_stroke(pos, r, Stroke::new(1.8, c));
            }
        }
    }
}

fn delete_selection(doc: &mut BpDoc) {
    for id in std::mem::take(&mut doc.selection) {
        doc.bp.remove_node(id);
    }
    doc.touched = true;
}

fn duplicate_selection(doc: &mut BpDoc, offset: Vec2) {
    let ids: Vec<BpNodeId> = doc.selection.iter().copied().collect();
    let new = doc.bp.duplicate_nodes(&ids, offset);
    doc.selection = new.into_iter().collect();
    doc.touched = true;
}

fn copy_selection(doc: &BpDoc) -> String {
    let mut sub = Blueprint::new();
    let mut map = std::collections::HashMap::new();
    for id in &doc.selection {
        if let Some(n) = doc.bp.node(*id) {
            let nid = sub.add_node(&n.op, n.pos);
            sub.node_mut(nid).expect("added").values = n.values.clone();
            map.insert(*id, nid);
        }
    }
    for l in &doc.bp.links {
        if let (Some(&a), Some(&b)) = (map.get(&l.from.node), map.get(&l.to.node)) {
            let _ = sub.connect(PinRef::new(a, &l.from.pin), PinRef::new(b, &l.to.pin));
        }
    }
    sub.to_json()
}

fn paste_nodes(doc: &mut BpDoc, clip: &str, at: Vec2) {
    let Ok(sub) = Blueprint::from_json(clip) else { return };
    let Some(min) = sub.nodes.iter().map(|n| n.pos).reduce(|a, b| a.min(b)) else { return };
    let mut map = std::collections::HashMap::new();
    for n in &sub.nodes {
        let nid = doc.bp.add_node(&n.op, n.pos - min + at);
        doc.bp.node_mut(nid).expect("added").values = n.values.clone();
        map.insert(n.id, nid);
    }
    for l in &sub.links {
        if let (Some(&a), Some(&b)) = (map.get(&l.from.node), map.get(&l.to.node)) {
            let _ = doc.bp.connect(PinRef::new(a, &l.from.pin), PinRef::new(b, &l.to.pin));
        }
    }
    doc.selection = map.into_values().collect();
    doc.touched = true;
}

fn comment_selection(doc: &mut BpDoc, layouts: &[(BpNodeId, &'static NodeDef, Layout)], view: &View) {
    let rects: Vec<Rect> = layouts.iter().filter(|(id, _, _)| doc.selection.contains(id)).map(|(_, _, l)| l.rect).collect();
    let Some(bb) = rects.into_iter().reduce(|a, b| a.union(b)) else { return };
    let min = view.to_world(bb.min) - vec2(20.0, 44.0);
    let max = view.to_world(bb.max) + vec2(20.0, 20.0);
    doc.bp.comments.push(Comment { pos: min, size: max - min, text: "Comment".into(), color: Color::rgba(0.55, 0.36, 0.96, 1.0) });
    doc.touched = true;
}

fn frame_all(doc: &BpDoc, rect: Rect, view: &mut View, selection_only: bool) {
    let nodes: Vec<Vec2> = doc
        .bp
        .nodes
        .iter()
        .filter(|n| !selection_only || doc.selection.contains(&n.id))
        .map(|n| n.pos)
        .collect();
    let (Some(min), Some(max)) = (nodes.iter().copied().reduce(Vec2::min), nodes.iter().copied().reduce(Vec2::max)) else { return };
    let max = max + vec2(260.0, 160.0);
    let size = max - min;
    view.zoom = (rect.width() / size.x).min(rect.height() / size.y).clamp(0.25, 1.2) * 0.9;
    let center = (min + max) * 0.5;
    view.pan = rect.size() * 0.5 - egui::vec2(center.x, center.y) * view.zoom;
}

fn node_menu(ctx: &egui::Context, doc: &mut BpDoc, g: &mut GraphUi) {
    let Some((nid, pos)) = g.menu else { return };
    let mut close = false;
    let area = egui::Area::new(egui::Id::unique("bp_node_menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(180.0);
            if let Some(def) = doc.bp.node(nid).and_then(|n| n.def()) {
                ui.label(RichText::new(def.title).strong());
                ui.label(RichText::new(def.description).small().color(theme::TEXT_DIM));
                ui.separator();
            }
            if ui.button("🗗  Duplicate        Ctrl+D").clicked() {
                duplicate_selection(doc, vec2(40.0, 40.0));
                close = true;
            }
            if ui.button("✂  Break all links").clicked() {
                doc.bp.links.retain(|l| l.from.node != nid && l.to.node != nid);
                doc.touched = true;
                close = true;
            }
            if ui.button("💬  Comment selection   C").clicked() {
                // Approximate: comment around the node.
                if let Some(n) = doc.bp.node(nid) {
                    let pos = n.pos;
                    doc.bp.comments.push(Comment { pos: pos - vec2(20.0, 44.0), size: vec2(300.0, 200.0), text: "Comment".into(), color: Color::rgba(0.55, 0.36, 0.96, 1.0) });
                    doc.touched = true;
                }
                close = true;
            }
            ui.separator();
            if ui.button(RichText::new("🗑  Delete            Del").color(theme::ERR)).clicked() {
                delete_selection(doc);
                close = true;
            }
        });
    });
    let fresh = ctx.cumulative_frame_nr() == g.opened_frame;
    if close || (!fresh && ctx.input(|i| i.pointer.any_pressed()) && !pointer_in(ctx, area.response.rect)) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        g.menu = None;
    }
}

fn compatible(def: &NodeDef, from: &Option<(PinRef, bool, PinKind)>) -> Option<&'static str> {
    let (_, out, kind) = from.as_ref()?;
    let pins = if *out { def.inputs } else { def.outputs };
    pins.iter()
        .find(|p| {
            !p.literal_only
                && match (kind, p.kind) {
                    (PinKind::Exec, PinKind::Exec) => true,
                    (PinKind::Data(a), PinKind::Data(b)) => {
                        if *out { b.accepts(*a) } else { a.accepts(b) }
                    }
                    _ => false,
                }
        })
        .map(|p| p.name)
}

fn palette(ctx: &egui::Context, doc: &mut BpDoc, g: &mut GraphUi, shared: &mut Shared) {
    let Some(pal) = &mut g.palette else { return };
    let mut chosen: Option<&'static NodeDef> = None;
    let mut close = false;
    let area = egui::Area::new(egui::Id::unique("bp_palette")).order(egui::Order::Foreground).fixed_pos(pal.screen).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(300.0);
            let title = match &pal.from {
                Some((_, true, k)) => format!("Add node accepting {}", kind_name(*k)),
                Some((_, false, k)) => format!("Add node producing {}", kind_name(*k)),
                None => "Add node".to_owned(),
            };
            ui.label(RichText::new(title).small().color(theme::TEXT_DIM));
            let r = ui.add(egui::TextEdit::singleline(&mut pal.search).hint_text("🔍 Search 140+ nodes").desired_width(f32::INFINITY));
            if !pal.focus || pal.focus_frames > 0 {
                r.request_focus();
                pal.focus = true;
                pal.focus_frames = pal.focus_frames.saturating_sub(1);
            }
            let q = pal.search.to_lowercase();
            let mut list: Vec<&'static NodeDef> = defs::LIBRARY
                .iter()
                .filter(|d| pal.from.is_none() || compatible(d, &pal.from).is_some())
                .filter(|d| q.is_empty() || d.title.to_lowercase().contains(&q) || d.category.to_lowercase().contains(&q) || d.description.to_lowercase().contains(&q) || d.id.contains(&q))
                .collect();
            if !q.is_empty() {
                list.sort_by_key(|d| (!d.title.to_lowercase().starts_with(&q), d.title));
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                chosen = list.first().copied();
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                let mut last = "";
                for d in list {
                    if q.is_empty() && d.category != last {
                        last = d.category;
                        ui.add_space(3.0);
                        ui.label(RichText::new(d.category.to_uppercase()).small().strong().color(theme::category_color(d.category)));
                    }
                    let r = ui.add(egui::Button::new(RichText::new(d.title)).frame(false).min_size(egui::vec2(ui.available_width(), 20.0)));
                    if r.clicked() {
                        chosen = Some(d);
                    }
                    r.on_hover_text(d.description);
                }
            });
        });
    });
    if let Some(def) = chosen {
        let id = doc.bp.add_node(def.id, pal.world - vec2(10.0, 15.0));
        if let Some((pin, out, _)) = &pal.from
            && let Some(target) = compatible(def, &pal.from)
        {
            let res = if *out { doc.bp.connect(pin.clone(), PinRef::new(id, target)) } else { doc.bp.connect(PinRef::new(id, target), pin.clone()) };
            if let Err(e) = res {
                shared.toast(LogLevel::Warn, e.to_string());
            }
        }
        // New "Get/Set Variable" nodes pick the first variable by default.
        if def.id.starts_with("var.")
            && let Some(v) = doc.bp.variables.first().map(|v| v.name.clone())
            && let Some(n) = doc.bp.node_mut(id)
        {
            n.values.insert("var".into(), Variant::str(v));
        }
        doc.selection = [id].into();
        doc.touched = true;
        close = true;
    }
    let fresh = ctx.cumulative_frame_nr() == g.opened_frame;
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (!fresh && ctx.input(|i| i.pointer.any_pressed()) && !pointer_in(ctx, area.response.rect)) {
        g.palette = None;
    }
}

fn kind_name(k: PinKind) -> &'static str {
    match k {
        PinKind::Exec => "execution",
        PinKind::Data(t) => t.name(),
    }
}

/// Popup editor for an unconnected input's literal value.
fn literal_editor(ctx: &egui::Context, doc: &mut BpDoc, g: &mut GraphUi, files: &[String], actions: &[String]) {
    let Some(edit) = &g.literal else { return };
    let Some(node) = doc.bp.node(edit.node) else {
        g.literal = None;
        return;
    };
    let Some(pin) = node.def().and_then(|d| d.input(&edit.pin)) else {
        g.literal = None;
        return;
    };
    let mut value = node.literal(pin.name);
    let vars: Vec<String> = doc.bp.variables.iter().map(|v| v.name.clone()).collect();
    let mut new_value: Option<Variant> = None;
    let mut close = false;
    let area = egui::Area::new(egui::Id::unique("bp_literal")).order(egui::Order::Foreground).fixed_pos(edit.pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(pin.name).small().color(theme::TEXT_DIM));
            let choices: Option<Vec<String>> = match pin.name {
                "var" => Some(vars.clone()),
                "action" | "negative" | "positive" | "left" | "right" | "up" | "down" => Some(actions.to_vec()),
                "scene" => Some(files.iter().filter(|f| f.ends_with(".vscn")).cloned().collect()),
                _ => None,
            };
            if let Some(choices) = choices {
                let current = value.to_string();
                for c in choices {
                    if ui.selectable_label(current == c, &c).clicked() {
                        new_value = Some(Variant::str(&c));
                        close = true;
                    }
                }
                ui.separator();
            }
            match pin.vtype() {
                VType::Any => {
                    let mut s = if value.is_nil() { String::new() } else { value.to_string() };
                    if ui.add(egui::TextEdit::singleline(&mut s).hint_text("number, true/false or text")).changed() {
                        new_value = Some(if let Ok(n) = s.trim().parse::<f64>() {
                            Variant::Number(n)
                        } else if s == "true" || s == "false" {
                            Variant::Bool(s == "true")
                        } else {
                            Variant::str(&s)
                        });
                    }
                }
                ty => {
                    let info = PropertyInfo { name: "literal", ty, hint: PropHint::None, group: "" };
                    if let Some(v) = crate::inspector::property_widget(ui, &info, &value, files) {
                        new_value = Some(v);
                    }
                }
            }
            if ui.small_button("Reset to default").clicked() {
                new_value = Some(pin.default.to_variant());
            }
        });
    });
    if let Some(v) = new_value {
        value = v;
        if let Some(n) = doc.bp.node_mut(edit.node) {
            n.values.insert(pin.name.to_owned(), value);
        }
        doc.touched = true;
    }
    let fresh = ctx.cumulative_frame_nr() == g.opened_frame;
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) || (!fresh && ctx.input(|i| i.pointer.any_pressed()) && !pointer_in(ctx, area.response.rect)) {
        g.literal = None;
    }
}

/// Right dock in the Blueprint workspace: variables, node details, library.
pub fn side_panel(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    let Some(idx) = p.active_bp.filter(|&i| i < p.blueprints.len()) else {
        theme::section(ui, "🔷", "Blueprint");
        ui.label(RichText::new("No blueprint open").color(theme::TEXT_DIM));
        return;
    };
    let doc = &mut p.blueprints[idx];
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        theme::section(ui, "🔷", "Blueprint");
        ui.label(RichText::new(&doc.path).monospace().small());
        ui.horizontal(|ui| {
            if doc.errors.is_empty() {
                ui.label(RichText::new("✔ compiles").color(theme::OK));
            } else {
                ui.label(RichText::new(format!("✖ {} error(s)", doc.errors.len())).color(theme::ERR));
            }
            if ui.small_button("Bytecode").clicked() {
                p.dialogs.bytecode = true;
            }
            if ui.small_button("✨ Tidy up").on_hover_text("Arrange nodes by execution and data flow").clicked() {
                doc.bp.auto_layout();
                doc.view.initialized = false;
                doc.touched = true;
            }
            if ui.small_button("💾 Save").clicked() {
                match doc.save(&p.dir) {
                    Ok(()) => shared.toast(LogLevel::Info, format!("Saved {}", doc.path)),
                    Err(e) => shared.toast(LogLevel::Error, e),
                }
            }
        });
        for e in &doc.errors {
            ui.label(RichText::new(format!("• {e}")).small().color(theme::ERR));
        }
        ui.add_space(8.0);

        theme::section(ui, "📦", "Variables");
        let mut remove = None;
        let mut add_node: Option<(&str, String)> = None;
        for (i, v) in doc.bp.variables.iter_mut().enumerate() {
            egui::Frame::NONE.fill(theme::SURFACE).corner_radius(6).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let c = type_color(PinKind::Data(v.ty));
                    ui.label(RichText::new("•").color(c));
                    if ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(100.0)).changed() {
                        doc.touched = true;
                    }
                    egui::ComboBox::from_id_salt(("vty", i)).selected_text(v.ty.name()).width(76.0).show_ui(ui, |ui| {
                        for t in [VType::Number, VType::Bool, VType::String, VType::Vec2, VType::Color, VType::Object, VType::Array, VType::Any] {
                            if ui.selectable_value(&mut v.ty, t, t.name()).changed() {
                                v.default = t.default_value();
                                doc.touched = true;
                            }
                        }
                    });
                    if ui.small_button("🗑").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("default").small().color(theme::TEXT_DIM));
                    let info = PropertyInfo { name: "default", ty: v.ty, hint: PropHint::None, group: "" };
                    if !matches!(v.ty, VType::Object | VType::Array | VType::Any)
                        && let Some(nv) = crate::inspector::property_widget(ui, &info, &v.default, &[])
                    {
                        v.default = nv;
                        doc.touched = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut v.exposed, RichText::new("exposed").small()).on_hover_text("Editable per node in the Inspector").changed() {
                        doc.touched = true;
                    }
                    if ui.small_button("Get").clicked() {
                        add_node = Some(("var.get", v.name.clone()));
                    }
                    if ui.small_button("Set").clicked() {
                        add_node = Some(("var.set", v.name.clone()));
                    }
                    if ui.small_button("+=").clicked() {
                        add_node = Some(("var.add", v.name.clone()));
                    }
                });
            });
            ui.add_space(3.0);
        }
        if let Some(i) = remove {
            doc.bp.variables.remove(i);
            doc.touched = true;
        }
        if ui.button("➕ Add variable").clicked() {
            let mut n = doc.bp.variables.len() + 1;
            while doc.bp.var(&format!("var{n}")).is_some() {
                n += 1;
            }
            doc.bp.add_variable(&format!("var{n}"), VType::Number, Variant::Number(0.0));
            doc.touched = true;
        }
        if let Some((op, var)) = add_node {
            let center = vec2(200.0, 200.0) - doc.view.pan / doc.view.zoom;
            let id = doc.bp.add(op, (center.x, center.y), &[("var", Variant::str(&var))]);
            doc.selection = [id].into();
            doc.touched = true;
        }
        ui.add_space(10.0);

        theme::section(ui, "ℹ", "Details");
        let selected: Vec<BpNodeId> = doc.selection.iter().copied().collect();
        if let [one] = selected.as_slice() {
            if let Some(def) = doc.bp.node(*one).and_then(|n| n.def()) {
                ui.label(RichText::new(def.title).strong().color(theme::category_color(def.category)));
                ui.label(RichText::new(def.description).small());
                ui.label(RichText::new(format!("{} · {}", def.category, def.id)).small().monospace().color(theme::TEXT_DIM));
                for pin in def.inputs.iter().chain(def.outputs) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("•").color(type_color(pin.kind)));
                        ui.label(RichText::new(pin.name).small());
                        ui.label(RichText::new(kind_name(pin.kind)).small().color(theme::TEXT_DIM));
                    });
                }
            }
        } else if selected.len() > 1 {
            ui.label(format!("{} nodes selected", selected.len()));
        } else {
            ui.label(RichText::new("Select a node to see details").small().color(theme::TEXT_DIM));
        }
        // Comments of the graph can be renamed here.
        if !doc.bp.comments.is_empty() {
            ui.add_space(8.0);
            theme::section(ui, "💬", "Comments");
            let mut remove = None;
            for (i, c) in doc.bp.comments.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut c.text).changed() {
                        doc.touched = true;
                    }
                    if ui.small_button("🗑").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                doc.bp.comments.remove(i);
                doc.touched = true;
            }
        }
        ui.add_space(10.0);

        theme::section(ui, "📚", "Node library");
        ui.add(egui::TextEdit::singleline(&mut p.graph_ui.library_search).hint_text("🔍 search").desired_width(f32::INFINITY));
        let q = p.graph_ui.library_search.to_lowercase();
        for cat in defs::categories() {
            let items: Vec<&NodeDef> = defs::LIBRARY.iter().filter(|d| d.category == cat && (q.is_empty() || d.title.to_lowercase().contains(&q))).collect();
            if items.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(RichText::new(format!("{cat} ({})", items.len())).color(theme::category_color(cat))).default_open(!q.is_empty()).show(ui, |ui| {
                for d in items {
                    if ui.add(egui::Button::new(d.title).frame(false)).on_hover_text(d.description).clicked() {
                        let center = vec2(240.0, 200.0) - doc.view.pan / doc.view.zoom;
                        let id = doc.bp.add_node(d.id, center);
                        doc.selection = BTreeSet::from([id]);
                        doc.touched = true;
                    }
                }
            });
        }
    });
}

pub fn bytecode_window(ctx: &egui::Context, p: &mut OpenProject) {
    if !p.dialogs.bytecode {
        return;
    }
    let mut open = true;
    let text = p.active_bp.and_then(|i| p.blueprints.get(i)).map(|d| {
        if d.disassembly.is_empty() { "; does not compile - see Problems".to_owned() } else { d.disassembly.clone() }
    });
    egui::Window::new("🔬 Blueprint bytecode").open(&mut open).default_size([520.0, 480.0]).show(ctx, |ui| {
        ui.label(RichText::new("Register VM bytecode (12-byte instructions) produced by the blueprint compiler.").small().color(theme::TEXT_DIM));
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text.unwrap_or_else(|| "; no blueprint open".into())).monospace()).wrap_mode(egui::TextWrapMode::Extend));
        });
    });
    if !open {
        p.dialogs.bytecode = false;
    }
}

/// True if the pointer is inside `rect` (children included, unlike `hovered`).
fn pointer_in(ctx: &egui::Context, rect: Rect) -> bool {
    ctx.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())).is_some_and(|p| rect.contains(p))
}
