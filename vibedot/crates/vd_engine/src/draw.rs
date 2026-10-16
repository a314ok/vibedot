//! Turns a scene tree into a renderer [`DrawList`] (Godot's canvas item
//! traversal). Used by the running game and by the editor viewport.

use std::sync::Arc;

use vd_core::{Affine2, Color, StringName, Vec2, vec2};
use vd_render::{Align, Blend, DrawCmd, DrawList, EnvSettings, FontFace, Light, Prim, QuadCmd, ShapeCode, TextCmd, TonemapMode};
use vd_scene::{BlendMode, NodeKind, SceneTree, ShapeType, TextAlign, Tonemap, WorldEnvironment};

pub fn env_settings(e: &WorldEnvironment) -> EnvSettings {
    EnvSettings {
        clear_color: e.clear_color,
        ambient: e.ambient_color.scaled(e.ambient),
        exposure: e.exposure,
        tonemap: match e.tonemap {
            Tonemap::Linear => TonemapMode::Linear,
            Tonemap::Reinhard => TonemapMode::Reinhard,
            Tonemap::Aces => TonemapMode::Aces,
            Tonemap::AgX => TonemapMode::AgX,
        },
        bloom: e.bloom,
        bloom_intensity: e.bloom_intensity,
        bloom_threshold: e.bloom_threshold,
        bloom_radius: e.bloom_radius,
        vignette: e.vignette,
        chromatic_aberration: e.chromatic_aberration,
        film_grain: e.film_grain,
        scanlines: e.scanlines,
        crt_curvature: e.crt_curvature,
        saturation: e.saturation,
        contrast: e.contrast,
        tint: e.tint,
        pixelate: e.pixelate,
    }
}

fn blend(b: BlendMode) -> Blend {
    match b {
        BlendMode::Alpha => Blend::Alpha,
        BlendMode::Additive => Blend::Additive,
    }
}

fn shape_code(s: ShapeType) -> ShapeCode {
    match s {
        ShapeType::Rect | ShapeType::RoundedRect => ShapeCode::RoundedRect,
        ShapeType::Circle => ShapeCode::Circle,
        ShapeType::Ring => ShapeCode::Ring,
        ShapeType::Triangle => ShapeCode::Triangle,
    }
}

/// Cheap deterministic value noise for light flicker.
fn flicker_noise(t: f32, seed: u64) -> f32 {
    let s = (seed % 997) as f32 * 0.37;
    ((t * 13.0 + s).sin() * 0.5 + (t * 7.3 + s * 2.0).sin() * 0.3 + (t * 23.0 + s).sin() * 0.2) * 0.5 + 0.5
}

/// Appends draw commands for every visible node inside the tree.
/// `out.view` must already be set; `time` animates flicker.
pub fn draw_tree(tree: &SceneTree, out: &mut DrawList, time: f32) {
    if let Some(env) = tree.environment() {
        out.env = env_settings(env);
    }
    // Pre-order walk carrying the accumulated z index (Godot `z_as_relative`).
    let mut stack: Vec<(vd_scene::NodeId, i32)> = vec![(tree.root(), 0)];
    while let Some((id, parent_z)) = stack.pop() {
        let Some(node) = tree.get(id) else { continue };
        if !node.is_visible_in_tree() {
            continue;
        }
        let z = parent_z + node.z_index;
        for &c in node.children().iter().rev() {
            stack.push((c, z));
        }
        let xf = node.global_transform();
        let modulate = node.global_modulate();
        match &node.kind {
            NodeKind::Sprite2D(s) => {
                let frames_h = s.hframes.max(1);
                let frames_v = s.vframes.max(1);
                let frame = s.frame % (frames_h * frames_v);
                let (col, row) = (frame % frames_h, frame / frames_h);
                let (fw, fh) = (1.0 / frames_h as f32, 1.0 / frames_v as f32);
                let mut uv = [col as f32 * fw, row as f32 * fh, (col + 1) as f32 * fw, (row + 1) as f32 * fh];
                if s.flip_h {
                    uv.swap(0, 2);
                }
                if s.flip_v {
                    uv.swap(1, 3);
                }
                let offset = if s.centered { Vec2::ZERO } else { s.size * 0.5 };
                let mut q = QuadCmd::shape(xf * Affine2::from_translation(offset), s.size, ShapeCode::Sprite, s.color * modulate);
                q.texture = StringName::new(&s.texture);
                q.uv = uv;
                q.emission = s.emission;
                q.blend = blend(s.blend);
                out.quad(z, q);
            }
            NodeKind::Shape2D(s) => {
                let mut q = QuadCmd::shape(xf, s.size, shape_code(s.shape), s.color * modulate);
                q.corner_radius = if s.shape == ShapeType::RoundedRect { s.corner_radius } else { 0.0 };
                q.thickness = s.thickness;
                q.softness = s.softness;
                q.emission = s.emission;
                q.blend = blend(s.blend);
                out.quad(z, q);
            }
            NodeKind::Label(l) => {
                let cmd = TextCmd {
                    xform: xf,
                    text: Arc::from(l.text.as_str()),
                    size: l.font_size,
                    color: l.color * modulate,
                    emission: l.emission,
                    align: match l.align {
                        TextAlign::Left => Align::Left,
                        TextAlign::Center => Align::Center,
                        TextAlign::Right => Align::Right,
                    },
                    face: if l.monospace { FontFace::Mono } else { FontFace::Sans },
                };
                if l.screen_space {
                    out.screen.push(DrawCmd { z, prim: Prim::Text(cmd) });
                } else {
                    out.text(z, cmd);
                }
            }
            NodeKind::PointLight2D(l) => {
                let scale = xf.matrix2.x_axis.length().max(xf.matrix2.y_axis.length());
                let flicker = if l.flicker > 0.0 { 1.0 - l.flicker * flicker_noise(time, id.to_bits()) } else { 1.0 };
                out.lights.push(Light {
                    position: xf.translation,
                    radius: l.radius * scale,
                    color: (l.color * modulate).scaled(l.energy * flicker),
                    falloff: l.falloff,
                });
            }
            NodeKind::Particles2D(p) => {
                let code = shape_code(p.shape);
                for part in &p.particles {
                    let t = (part.age / part.life.max(1e-4)).clamp(0.0, 1.0);
                    let size = vd_core::math::lerp(p.size_start, p.size_end, t) * part.size_jitter;
                    if size <= 0.01 {
                        continue;
                    }
                    let color = p.color_start.lerp(p.color_end, t) * modulate;
                    let pos = if p.world_space { part.pos } else { xf.transform_point2(part.pos) };
                    let pxf = Affine2::from_angle_translation(part.angle, pos);
                    let mut q = QuadCmd::shape(pxf, Vec2::splat(size), code, color);
                    q.emission = p.emission;
                    q.blend = blend(p.blend);
                    q.softness = if code == ShapeCode::Circle { size * 0.15 } else { 0.0 };
                    out.quad(z, q);
                }
            }
            NodeKind::Node(_)
            | NodeKind::Node2D(_)
            | NodeKind::Camera2D(_)
            | NodeKind::Body2D(_)
            | NodeKind::Timer(_)
            | NodeKind::WorldEnvironment(_) => {}
        }
    }
}

/// Collision shape outlines (editor "Visible Collision Shapes" debug draw).
pub fn draw_collision_shapes(tree: &SceneTree, out: &mut DrawList) {
    for id in tree.all_nodes() {
        let Some(node) = tree.get(id) else { continue };
        let NodeKind::Body2D(b) = &node.kind else {
            continue;
        };
        let pos = node.global_position();
        let color = match b.body_type {
            vd_scene::BodyType::Area => Color::rgba(0.3, 1.0, 0.5, 0.5),
            vd_scene::BodyType::Static => Color::rgba(0.4, 0.6, 1.0, 0.5),
            _ => Color::rgba(1.0, 0.6, 0.2, 0.5),
        };
        let scale = node.global_transform().matrix2.x_axis.length().max(node.global_transform().matrix2.y_axis.length());
        let mut q = match b.shape {
            vd_scene::CollisionShape::Rect => {
                QuadCmd::shape(Affine2::from_translation(pos), b.size * node.transform.scale.abs(), ShapeCode::RoundedRect, color)
            }
            vd_scene::CollisionShape::Circle => {
                QuadCmd::shape(Affine2::from_translation(pos), Vec2::splat(b.radius * 2.0 * scale), ShapeCode::Ring, color)
            }
        };
        q.thickness = 2.0;
        if b.shape == vd_scene::CollisionShape::Rect {
            // Outline via ring-like rounded rect: draw 4 thin bars.
            let size = b.size * node.transform.scale.abs();
            let half = size * 0.5;
            for (off, sz) in [
                (vec2(0.0, -half.y), vec2(size.x, 2.0)),
                (vec2(0.0, half.y), vec2(size.x, 2.0)),
                (vec2(-half.x, 0.0), vec2(2.0, size.y)),
                (vec2(half.x, 0.0), vec2(2.0, size.y)),
            ] {
                out.quad(1000, QuadCmd::shape(Affine2::from_translation(pos + off), sz, ShapeCode::RoundedRect, color));
            }
            continue;
        }
        q.emission = 0.0;
        out.quad(1000, q);
    }
}
