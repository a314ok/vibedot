//! GPU smoke test: renders a scene off-screen and checks pixels.
//! Skips (passes) when no GPU adapter is available.

use std::sync::Arc;

use vd_core::{Affine2, Color, StringName, vec2};
use vd_render::*;

fn scene() -> DrawList {
    let mut list = DrawList { view: View2D { center: vec2(0.0, 0.0), zoom: 1.0, rotation: 0.0 }, ..Default::default() };
    list.env.ambient = Color::rgb(0.25, 0.25, 0.35);
    list.env.vignette = 0.4;
    list.env.bloom = std::env::var("VD_NOBLOOM").is_err();
    list.env.chromatic_aberration = 1.0;
    // Floor.
    let mut floor = QuadCmd::shape(
        Affine2::from_translation(vec2(0.0, 140.0)),
        vec2(600.0, 40.0),
        ShapeCode::RoundedRect,
        Color::from_hex("#3a3f5c").unwrap(),
    );
    floor.corner_radius = 10.0;
    list.quad(0, floor);
    // A glowing ring, circle, triangle.
    let mut ring =
        QuadCmd::shape(Affine2::from_translation(vec2(-200.0, 0.0)), vec2(120.0, 120.0), ShapeCode::Ring, Color::rgb(0.2, 0.9, 1.0));
    ring.thickness = 10.0;
    ring.emission = 4.0;
    list.quad(1, ring);
    list.quad(
        1,
        QuadCmd::shape(Affine2::from_translation(vec2(0.0, 0.0)), vec2(100.0, 100.0), ShapeCode::Circle, Color::rgb(1.0, 0.4, 0.2)),
    );
    list.quad(
        1,
        QuadCmd::shape(
            Affine2::from_scale_angle_translation(vec2(1.0, 1.0), 0.5, vec2(200.0, 0.0)),
            vec2(100.0, 80.0),
            ShapeCode::Triangle,
            Color::rgb(0.6, 1.0, 0.3),
        ),
    );
    // Missing texture → checkerboard sprite.
    let mut spr = QuadCmd::shape(Affine2::from_translation(vec2(0.0, -120.0)), vec2(64.0, 64.0), ShapeCode::Sprite, Color::WHITE);
    spr.texture = StringName::new("missing.png");
    list.quad(2, spr);
    // Additive spark.
    let mut spark =
        QuadCmd::shape(Affine2::from_translation(vec2(-200.0, -120.0)), vec2(40.0, 40.0), ShapeCode::Circle, Color::rgb(1.0, 0.8, 0.2));
    spark.blend = Blend::Additive;
    spark.softness = 12.0;
    spark.emission = 3.0;
    list.quad(3, spark);
    list.text(
        5,
        TextCmd {
            xform: Affine2::from_translation(vec2(0.0, 80.0)),
            text: Arc::from("Vibedot"),
            size: 48.0,
            color: Color::WHITE,
            emission: 1.5,
            align: Align::Center,
            face: FontFace::Sans,
        },
    );
    list.lights.push(Light { position: vec2(0.0, 0.0), radius: 300.0, color: Color::rgb(1.0, 0.8, 0.6).scaled(1.5), falloff: 2.0 });
    list.screen.push(DrawCmd {
        z: 0,
        prim: Prim::Text(TextCmd {
            xform: Affine2::from_translation(vec2(20.0, 20.0)),
            text: Arc::from("SCORE 42"),
            size: 20.0,
            color: Color::YELLOW,
            emission: 0.0,
            align: Align::Left,
            face: FontFace::Mono,
        }),
    });
    list
}

#[test]
fn renders_scene_offscreen() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter - skipping");
        return;
    };
    let mut renderer = Renderer::new(&device, &queue, Arc::new(|_| None));
    let size = (640, 360);
    let px = render_to_rgba(&mut renderer, &scene(), size);
    assert_eq!(px.len(), 640 * 360 * 4);
    if let Ok(dir) = std::env::var("VD_SCREENSHOT_DIR") {
        let path = std::path::Path::new(&dir).join("render_test.png");
        image::save_buffer(&path, &px, size.0, size.1, image::ColorType::Rgba8).unwrap();
        eprintln!("saved {}", path.display());
    }
    let at = |x: u32, y: u32| {
        let i = ((y * size.0 + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    // Circle center (screen 320,180) is orange-ish and lit.
    let c = at(320, 180);
    assert!(c[0] > 150 && c[0] > c[2], "circle {c:?}");
    // Ring glows cyan (screen 120,180 is ring center -> check its edge at x=65).
    let r = at(120 - 55, 180);
    assert!(r[2] > 150 && r[1] > 150, "ring {r:?}");
    // Corner is dark (vignette + ambient).
    let k = at(2, 358);
    assert!(k.iter().all(|&v| v < 40), "corner {k:?}");
    // Second frame reuses resources and stays stable.
    let px2 = render_to_rgba(&mut renderer, &scene(), size);
    assert_eq!(px.len(), px2.len());
}
