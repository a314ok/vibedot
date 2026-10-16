//! The 2D viewport: WYSIWYG scene rendering (the same renderer the game
//! uses, so bloom and lights are visible while editing), grid, camera
//! frame, selection, move/rotate gizmos, snapping and play-in-editor.

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, RichText, Stroke};
use vd_core::{Affine2, Rect2, Vec2, vec2};
use vd_egui::{GameViewport, feed_input, rect_pixels, theme};
use vd_render::{DrawCmd, DrawList, Prim, View2D};
use vd_scene::{Node, NodeId, NodeKind, SceneTree};

use crate::app::{OpenProject, Request, Shared, Tool};
use crate::files::DragFile;

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragMode {
    Free,
    AxisX,
    AxisY,
    Rotate,
    Pan,
}

#[derive(Clone, Debug)]
struct Drag {
    mode: DragMode,
    start_world: Vec2,
    start_pos: Vec<(NodeId, Vec2)>,
    start_rot: Vec<(NodeId, f32)>,
    pivot: Vec2,
}

#[derive(Default)]
pub struct GizmoState {
    drag: Option<Drag>,
    hover: Option<DragMode>,
}

/// World ↔ screen (egui points) mapping for the current frame.
struct Xf {
    rect: egui::Rect,
    ppp: f32,
    view: View2D,
    size: Vec2,
}

impl Xf {
    fn w2s(&self, w: Vec2) -> Pos2 {
        let px = self.view.world_to_screen(self.size).transform_point2(w);
        self.rect.min + egui::vec2(px.x, px.y) / self.ppp
    }
    fn s2w(&self, p: Pos2) -> Vec2 {
        let local = (p - self.rect.min) * self.ppp;
        self.view.screen_to_world(self.size, vec2(local.x, local.y))
    }
    /// Screen points per world unit.
    fn scale(&self) -> f32 {
        self.view.zoom / self.ppp
    }
}

/// Local-space bounds used for picking and selection outlines.
pub fn local_bounds(node: &Node) -> Option<Rect2> {
    let icon = Rect2::from_center_size(Vec2::ZERO, Vec2::splat(28.0));
    Some(match &node.kind {
        NodeKind::Sprite2D(s) => {
            if s.centered {
                Rect2::from_center_size(Vec2::ZERO, s.size)
            } else {
                Rect2::new(Vec2::ZERO, s.size)
            }
        }
        NodeKind::Shape2D(s) => Rect2::from_center_size(Vec2::ZERO, s.size),
        NodeKind::Label(l) => {
            let face = if l.monospace { vd_render::FontFace::Mono } else { vd_render::FontFace::Sans };
            let size = vd_render::text::measure(&l.text, l.font_size, face);
            let x0 = match l.align {
                vd_scene::TextAlign::Left => 0.0,
                vd_scene::TextAlign::Center => -size.x / 2.0,
                vd_scene::TextAlign::Right => -size.x,
            };
            Rect2::new(vec2(x0, -size.y / 2.0), size)
        }
        NodeKind::Body2D(b) => match b.shape {
            vd_scene::CollisionShape::Rect => Rect2::from_center_size(Vec2::ZERO, b.size),
            vd_scene::CollisionShape::Circle => Rect2::from_center_size(Vec2::ZERO, Vec2::splat(b.radius * 2.0)),
        },
        NodeKind::Particles2D(p) => Rect2::from_center_size(Vec2::ZERO, Vec2::splat((p.emit_radius * 2.0).max(28.0))),
        NodeKind::Camera2D(_) | NodeKind::PointLight2D(_) | NodeKind::Node2D(_) => icon,
        NodeKind::Node(_) | NodeKind::Timer(_) | NodeKind::WorldEnvironment(_) => return None,
    })
}

fn z_of(tree: &SceneTree, id: NodeId) -> i32 {
    let mut z = 0;
    let mut cur = Some(id);
    while let Some(c) = cur {
        z += tree.get(c).map_or(0, |n| n.z_index);
        cur = tree.get_parent(c);
    }
    z
}

/// Topmost node under a world point.
fn pick(tree: &SceneTree, root: NodeId, world: Vec2) -> Option<NodeId> {
    let mut best: Option<(i32, usize, NodeId)> = None;
    for (order, id) in tree.preorder(root).into_iter().enumerate() {
        let node = tree.get(id)?;
        if !node.is_visible_in_tree() || id == root {
            continue;
        }
        let Some(b) = local_bounds(node) else {
            continue;
        };
        let local = node.global_transform().inverse().transform_point2(world);
        if b.grow(2.0).contains(local) {
            let key = (z_of(tree, id), order, id);
            if best.is_none_or(|(z, o, _)| (key.0, key.1) > (z, o)) {
                best = Some(key);
            }
        }
    }
    best.map(|b| b.2)
}

/// The rectangle the game window will show (camera frame) in world space.
fn camera_frame(p: &OpenProject) -> (Rect2, f32) {
    let win = vec2(p.settings.window.width as f32, p.settings.window.height as f32);
    let tree = &p.scene.tree;
    let cam = tree.all_nodes().into_iter().find_map(|id| match &tree.get(id)?.kind {
        NodeKind::Camera2D(c) if c.current => Some((tree.global_position(id), c.zoom.max(0.01))),
        _ => None,
    });
    match cam {
        Some((pos, zoom)) => (Rect2::from_center_size(pos, win / zoom), zoom),
        None => (Rect2::new(Vec2::ZERO, win), 1.0),
    }
}

pub fn show(ui: &mut egui::Ui, frame: &mut eframe::Frame, p: &mut OpenProject, vp: &mut GameViewport, shared: &mut Shared, dt: f32) {
    let rect = ui.available_rect_before_wrap();
    let id = ui.scope_id().with("viewport");
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let size_px = rect_pixels(&ctx, rect);
    let Some(rs) = frame.wgpu_render_state() else {
        ui.label("wgpu not available");
        return;
    };

    // ------------------------------------------------------------ play mode
    if let Some(play) = &mut p.play {
        let game = &mut play.game;
        game.host.viewport = vec2(size_px.0 as f32, size_px.1 as f32);
        game.host.input.begin_frame();
        if resp.clicked() {
            resp.request_focus();
        }
        let focus = !ctx.egui_wants_keyboard_input() || resp.has_focus();
        feed_input(&ctx, rect, &mut game.host.input, focus);
        if !play.paused {
            game.update(dt);
        }
        game.draw(&mut play.list);
        vp.render(rs, &play.list, size_px);
        vp.paint(ui, rect);
        let badge = if play.paused { "⏸ PAUSED" } else { "• PLAYING" };
        let painter = ui.painter_at(rect);
        let r = egui::Rect::from_min_size(rect.right_top() + egui::vec2(-262.0, 12.0), egui::vec2(250.0, 26.0));
        painter.rect_filled(r, 13, Color32::from_black_alpha(150));
        painter.text(
            r.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{badge}   F5 to stop"),
            egui::FontId::proportional(13.0),
            if play.paused { theme::WARN } else { theme::OK },
        );
        return;
    }

    // ------------------------------------------------------------ edit mode
    let ppp = ctx.pixels_per_point();
    let mut xf = Xf {
        rect,
        ppp,
        view: View2D { center: p.cam.center, zoom: p.cam.zoom * ppp, rotation: 0.0 },
        size: vec2(size_px.0 as f32, size_px.1 as f32),
    };

    // Zoom around the cursor.
    if resp.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 300.0);
        if scroll.abs() > 0.01
            && let Some(mouse) = ctx.input(|i| i.pointer.hover_pos())
        {
            let before = xf.s2w(mouse);
            p.cam.zoom = (p.cam.zoom * (1.0 + scroll * 0.0015)).clamp(0.05, 20.0);
            xf.view.zoom = p.cam.zoom * ppp;
            let after = xf.s2w(mouse);
            p.cam.center += before - after;
            xf.view.center = p.cam.center;
        }
    }

    let tree = &mut p.scene.tree;
    let mut list = DrawList { view: xf.view, time: shared.time as f32, ..Default::default() };
    list.time = shared.time as f32;
    let t = list.time;
    vd_engine::draw_tree(tree, &mut list, t);
    // HUD preview: screen-space items are shown inside the camera frame.
    let (frame_rect, frame_zoom) = camera_frame(p);
    for cmd in std::mem::take(&mut list.screen) {
        if let Prim::Text(mut t) = cmd.prim {
            t.xform = Affine2::from_translation(frame_rect.min) * Affine2::from_scale(Vec2::splat(1.0 / frame_zoom)) * t.xform;
            list.world.push(DrawCmd { z: 1000 + cmd.z, prim: Prim::Text(t) });
        }
    }
    if p.show_colliders {
        vd_engine::draw_collision_shapes(&p.scene.tree, &mut list);
    }
    vp.render(rs, &list, size_px);
    vp.paint(ui, rect);

    let painter = ui.painter_at(rect);
    if p.show_grid {
        draw_grid(&painter, &xf, p.grid);
    }
    // Camera frame.
    let fr = egui::Rect::from_min_max(xf.w2s(frame_rect.min), xf.w2s(frame_rect.max()));
    painter.rect_stroke(fr, 0, Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.85)), egui::StrokeKind::Middle);
    painter.text(
        fr.left_top() + egui::vec2(4.0, -14.0),
        egui::Align2::LEFT_CENTER,
        "game view",
        egui::FontId::proportional(11.0),
        theme::ACCENT,
    );

    // Icons for non-visual nodes and selection outlines.
    let root = p.scene.root();
    for nid in p.scene.tree.preorder(root) {
        let node = p.scene.tree.get(nid).expect("listed");
        let pos = xf.w2s(node.global_position());
        let icon = match &node.kind {
            NodeKind::Camera2D(_) => Some("🎥"),
            NodeKind::PointLight2D(_) => Some("💡"),
            NodeKind::Particles2D(_) => Some("✨"),
            _ => None,
        };
        if let Some(i) = icon
            && rect.contains(pos)
        {
            painter.text(pos, egui::Align2::CENTER_CENTER, i, egui::FontId::proportional(16.0), Color32::from_white_alpha(200));
        }
        if p.scene.is_selected(nid) {
            if let Some(b) = local_bounds(node) {
                let g = node.global_transform();
                let corners = [b.min, vec2(b.max().x, b.min.y), b.max(), vec2(b.min.x, b.max().y)].map(|c| xf.w2s(g.transform_point2(c)));
                let primary = p.scene.primary() == Some(nid);
                let stroke = Stroke::new(if primary { 2.0 } else { 1.0 }, if primary { theme::ACCENT_2 } else { theme::ACCENT });
                for i in 0..4 {
                    painter.line_segment([corners[i], corners[(i + 1) % 4]], stroke);
                }
            }
            if let NodeKind::PointLight2D(l) = &node.kind {
                painter.circle_stroke(pos, l.radius * xf.scale(), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 220, 120, 120)));
            }
        }
    }

    // ------------------------------------------------------------ gizmo + input
    let mouse = ctx.input(|i| i.pointer.hover_pos());
    let mouse_world = mouse.map(|m| xf.s2w(m));
    let primary = p.scene.primary().filter(|&s| p.scene.tree.get(s).is_some_and(|n| n.kind.is_2d()));
    let gizmo_origin = primary.map(|s| xf.w2s(p.scene.tree.global_position(s)));
    // Hover detection for handles.
    p.gizmo.hover = None;
    if let (Some(o), Some(m)) = (gizmo_origin, mouse)
        && p.gizmo.drag.is_none()
    {
        let d = m - o;
        if p.tool == Tool::Rotate {
            if (d.length() - 70.0).abs() < 8.0 {
                p.gizmo.hover = Some(DragMode::Rotate);
            }
        } else if p.tool != Tool::Pan {
            if d.x.abs() < 8.0 && d.y.abs() < 8.0 {
                p.gizmo.hover = Some(DragMode::Free);
            } else if d.x > 8.0 && d.x < 70.0 && d.y.abs() < 6.0 {
                p.gizmo.hover = Some(DragMode::AxisX);
            } else if d.y < -8.0 && d.y > -70.0 && d.x.abs() < 6.0 {
                p.gizmo.hover = Some(DragMode::AxisY);
            }
        }
    }
    if let Some(o) = gizmo_origin {
        draw_gizmo(&painter, o, p.tool, p.gizmo.hover.or(p.gizmo.drag.as_ref().map(|d| d.mode)));
    }

    let space_down = ctx.input(|i| i.key_down(egui::Key::Space)) && !ctx.egui_wants_keyboard_input();
    let pan_button = resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary);
    if pan_button || (resp.dragged_by(egui::PointerButton::Primary) && (space_down || p.tool == Tool::Pan)) {
        let d = resp.drag_delta();
        p.cam.center -= vec2(d.x, d.y) / (p.cam.zoom);
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(mw) = mouse_world {
            let press = ctx.input(|i| i.pointer.press_origin()).map(|o| xf.s2w(o)).unwrap_or(mw);
            let mode = p.gizmo.hover.or_else(|| {
                // Dragging a node directly moves it (selecting it first).
                let hit = pick(&p.scene.tree, root, press)?;
                if !p.scene.is_selected(hit) {
                    p.scene.select(hit, false);
                }
                Some(if p.tool == Tool::Rotate { DragMode::Rotate } else { DragMode::Free })
            });
            if let Some(mode) = mode {
                let sel: Vec<NodeId> = p.scene.selection.iter().copied().filter(|&s| s != root && p.scene.tree.contains(s)).collect();
                let pivot = p.scene.primary().map_or(press, |s| p.scene.tree.global_position(s));
                p.gizmo.drag = Some(Drag {
                    mode,
                    start_world: press,
                    start_pos: sel.iter().map(|&s| (s, p.scene.tree.global_position(s))).collect(),
                    start_rot: sel.iter().map(|&s| (s, p.scene.tree.get(s).map_or(0.0, |n| n.transform.rotation))).collect(),
                    pivot,
                });
            } else {
                p.gizmo.drag = Some(Drag { mode: DragMode::Pan, start_world: press, start_pos: vec![], start_rot: vec![], pivot: press });
            }
        }
    } else if resp.dragged_by(egui::PointerButton::Primary)
        && let (Some(drag), Some(mw)) = (&p.gizmo.drag, mouse_world)
    {
        let snap = p.snap ^ ctx.input(|i| i.modifiers.command);
        let delta = mw - drag.start_world;
        match drag.mode {
            DragMode::Free | DragMode::AxisX | DragMode::AxisY => {
                let delta = match drag.mode {
                    DragMode::AxisX => vec2(delta.x, 0.0),
                    DragMode::AxisY => vec2(0.0, delta.y),
                    _ => delta,
                };
                for &(nid, start) in &drag.start_pos {
                    let mut target = start + delta;
                    if snap {
                        target = (target / p.grid).round() * p.grid;
                    }
                    p.scene.tree.set_global_position(nid, target);
                }
                p.scene.touched = true;
            }
            DragMode::Rotate => {
                let a0 = (drag.start_world - drag.pivot).to_angle();
                let a1 = (mw - drag.pivot).to_angle();
                let mut da = a1 - a0;
                if ctx.input(|i| i.modifiers.shift) || snap {
                    let step = 15f32.to_radians();
                    da = (da / step).round() * step;
                }
                for &(nid, r0) in &drag.start_rot {
                    if let Some(n) = p.scene.tree.get_mut(nid) {
                        n.transform.rotation = r0 + da;
                    }
                }
                p.scene.touched = true;
            }
            DragMode::Pan => {
                // Box selection could go here; empty-space drags pan.
                let d = resp.drag_delta();
                p.cam.center -= vec2(d.x, d.y) / p.cam.zoom;
            }
        }
    }
    if resp.drag_stopped() {
        p.gizmo.drag = None;
    }
    if resp.clicked()
        && let Some(mw) = mouse_world
    {
        let additive = ctx.input(|i| i.modifiers.shift || i.modifiers.command);
        match pick(&p.scene.tree, root, mw) {
            Some(hit) => p.scene.select(hit, additive),
            None => {
                if !additive {
                    p.scene.selection.clear();
                }
            }
        }
    }
    if resp.hovered() && !ctx.egui_wants_keyboard_input() {
        let (f, w, e, r, q) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::W),
                i.key_pressed(egui::Key::E),
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::Q),
            )
        });
        if f {
            if let Some(s) = p.scene.primary() {
                p.cam.center = p.scene.tree.global_position(s);
            } else {
                p.frame_scene();
            }
        }
        if q {
            p.tool = Tool::Select;
        }
        if w {
            p.tool = Tool::Move;
        }
        if e || r {
            p.tool = Tool::Rotate;
        }
    }

    // Drop files from the FileSystem dock.
    if let Some(payload) = resp.dnd_release_payload::<DragFile>()
        && let Some(mw) = mouse_world
    {
        let path = payload.0.clone();
        let at = if p.snap { (mw / p.grid).round() * p.grid } else { mw };
        if path.ends_with(".png") {
            shared.request(Request::AddSprite { texture: path, at });
        } else if path.ends_with(".vscn") {
            shared.request(Request::InstanceScene { path, at });
        }
    }
    if resp.dnd_hover_payload::<DragFile>().is_some() {
        painter.rect_stroke(rect.shrink(2.0), 4, Stroke::new(2.0, theme::ACCENT_2), egui::StrokeKind::Inside);
    }

    toolbar(ui, rect, p);
    if let Some(mw) = mouse_world
        && resp.hovered()
    {
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("x {:.0}  y {:.0}", mw.x, mw.y),
            egui::FontId::monospace(11.0),
            theme::TEXT_DIM,
        );
    }
    let _ = Arc::clone(&p.assets);
}

fn draw_grid(painter: &egui::Painter, xf: &Xf, grid: f32) {
    let scale = xf.scale();
    let mut step = grid;
    while step * scale < 12.0 {
        step *= 2.0;
    }
    let tl = xf.s2w(xf.rect.min);
    let br = xf.s2w(xf.rect.max);
    let minor = Color32::from_white_alpha(10);
    let major = Color32::from_white_alpha(22);
    let mut x = (tl.x / step).floor() * step;
    let mut guard = 0;
    while x <= br.x && guard < 1000 {
        let is_major = ((x / step).round() as i64) % 8 == 0;
        let sx = xf.w2s(vec2(x, 0.0)).x;
        let c = if x.abs() < step * 0.5 {
            theme::OK.gamma_multiply(0.45)
        } else if is_major {
            major
        } else {
            minor
        };
        painter.line_segment([Pos2::new(sx, xf.rect.top()), Pos2::new(sx, xf.rect.bottom())], Stroke::new(1.0, c));
        x += step;
        guard += 1;
    }
    let mut y = (tl.y / step).floor() * step;
    guard = 0;
    while y <= br.y && guard < 1000 {
        let is_major = ((y / step).round() as i64) % 8 == 0;
        let sy = xf.w2s(vec2(0.0, y)).y;
        let c = if y.abs() < step * 0.5 {
            theme::ERR.gamma_multiply(0.45)
        } else if is_major {
            major
        } else {
            minor
        };
        painter.line_segment([Pos2::new(xf.rect.left(), sy), Pos2::new(xf.rect.right(), sy)], Stroke::new(1.0, c));
        y += step;
        guard += 1;
    }
}

fn draw_gizmo(painter: &egui::Painter, o: Pos2, tool: Tool, hot: Option<DragMode>) {
    let hl = |m: DragMode, c: Color32| if hot == Some(m) { Color32::WHITE } else { c };
    if tool == Tool::Rotate {
        let c = hl(DragMode::Rotate, theme::ACCENT_2);
        painter.circle_stroke(o, 70.0, Stroke::new(if hot == Some(DragMode::Rotate) { 3.0 } else { 2.0 }, c));
        painter.circle_filled(o, 3.0, c);
        return;
    }
    if tool == Tool::Pan {
        return;
    }
    let x = hl(DragMode::AxisX, theme::ERR);
    let y = hl(DragMode::AxisY, theme::OK);
    painter.line_segment([o + egui::vec2(8.0, 0.0), o + egui::vec2(62.0, 0.0)], Stroke::new(2.5, x));
    painter.add(egui::Shape::convex_polygon(
        vec![o + egui::vec2(72.0, 0.0), o + egui::vec2(60.0, -6.0), o + egui::vec2(60.0, 6.0)],
        x,
        Stroke::NONE,
    ));
    painter.line_segment([o + egui::vec2(0.0, -8.0), o + egui::vec2(0.0, -62.0)], Stroke::new(2.5, y));
    painter.add(egui::Shape::convex_polygon(
        vec![o + egui::vec2(0.0, -72.0), o + egui::vec2(-6.0, -60.0), o + egui::vec2(6.0, -60.0)],
        y,
        Stroke::NONE,
    ));
    let c = hl(DragMode::Free, theme::ACCENT_2);
    painter.rect_filled(egui::Rect::from_center_size(o, egui::vec2(12.0, 12.0)), 3, c.gamma_multiply(0.85));
}

fn toolbar(ui: &mut egui::Ui, rect: egui::Rect, p: &mut OpenProject) {
    let bar = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(rect.width() - 20.0, 32.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        egui::Frame::NONE
            .fill(Color32::from_rgba_unmultiplied(16, 16, 24, 220))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (tool, icon, tip) in [
                        (Tool::Select, "⬉", "Select (Q)"),
                        (Tool::Move, "↔", "Move (W)"),
                        (Tool::Rotate, "⟳", "Rotate (E)"),
                        (Tool::Pan, "✋", "Pan (or hold Space / middle mouse)"),
                    ] {
                        let active = p.tool == tool;
                        let b = egui::Button::new(RichText::new(icon).size(15.0))
                            .fill(if active { theme::ACCENT } else { Color32::TRANSPARENT })
                            .corner_radius(6);
                        if ui.add(b).on_hover_text(tip).clicked() {
                            p.tool = tool;
                        }
                    }
                    ui.separator();
                    ui.toggle_value(&mut p.snap, "📌 Snap").on_hover_text("Snap to grid (hold Ctrl to toggle)");
                    ui.add(egui::DragValue::new(&mut p.grid).range(1.0..=512.0).speed(1.0).suffix(" px")).on_hover_text("Grid size");
                    ui.toggle_value(&mut p.show_grid, "☰").on_hover_text("Show grid");
                    ui.toggle_value(&mut p.show_colliders, "○").on_hover_text("Show collision shapes");
                    ui.separator();
                    if ui.button("1:1").on_hover_text("Reset zoom").clicked() {
                        p.cam.zoom = 1.0;
                    }
                    if ui.button("⛶").on_hover_text("Frame selection / scene (F)").clicked() {
                        if let Some(s) = p.scene.primary() {
                            p.cam.center = p.scene.tree.global_position(s);
                        } else {
                            p.frame_scene();
                        }
                    }
                });
            });
    });
}
