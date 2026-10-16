//! Backend-agnostic draw list - what the engine hands to the renderer each
//! frame (Godot's `RenderingServer` canvas items, flattened).

use std::sync::Arc;

use vd_core::{Affine2, Color, StringName, Vec2};

use crate::text::{Align, FontFace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Blend {
    #[default]
    Alpha,
    /// Unlit, adds light (lasers, sparks). Only feeds the emissive buffer.
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum ShapeCode {
    /// Textured rectangle.
    #[default]
    Sprite = 0,
    RoundedRect = 1,
    Circle = 2,
    Ring = 3,
    Triangle = 4,
}

#[derive(Clone, Debug)]
pub struct QuadCmd {
    /// Maps the unit square `[-0.5, 0.5]²` to world (or screen) space,
    /// i.e. it already includes the quad size.
    pub xform: Affine2,
    /// Size in local pixels (for SDF anti-aliasing and corner radii).
    pub size: Vec2,
    pub shape: ShapeCode,
    pub color: Color,
    /// Texture path; empty = plain white.
    pub texture: StringName,
    pub uv: [f32; 4],
    pub corner_radius: f32,
    pub thickness: f32,
    pub softness: f32,
    pub emission: f32,
    pub blend: Blend,
}

impl QuadCmd {
    pub fn shape(xform: Affine2, size: Vec2, shape: ShapeCode, color: Color) -> Self {
        Self {
            xform: xform * Affine2::from_scale(size),
            size,
            shape,
            color,
            texture: StringName::EMPTY,
            uv: [0.0, 0.0, 1.0, 1.0],
            corner_radius: 0.0,
            thickness: 0.0,
            softness: 0.0,
            emission: 0.0,
            blend: Blend::Alpha,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextCmd {
    /// Transform of the text anchor (block is centered vertically on it,
    /// horizontally according to `align`).
    pub xform: Affine2,
    pub text: Arc<str>,
    pub size: f32,
    pub color: Color,
    pub emission: f32,
    pub align: Align,
    pub face: FontFace,
}

#[derive(Clone, Debug)]
pub enum Prim {
    Quad(QuadCmd),
    Text(TextCmd),
}

#[derive(Clone, Debug)]
pub struct DrawCmd {
    pub z: i32,
    pub prim: Prim,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub position: Vec2,
    pub radius: f32,
    /// Color × energy (HDR).
    pub color: Color,
    pub falloff: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TonemapMode {
    Linear = 0,
    Reinhard = 1,
    Aces = 2,
    AgX = 3,
}

/// Post-processing and lighting settings (from `WorldEnvironment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvSettings {
    pub clear_color: Color,
    pub ambient: Color,
    pub exposure: f32,
    pub tonemap: TonemapMode,
    pub bloom: bool,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub bloom_radius: f32,
    pub vignette: f32,
    pub chromatic_aberration: f32,
    pub film_grain: f32,
    pub scanlines: f32,
    pub crt_curvature: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub tint: Color,
    pub pixelate: f32,
}

impl Default for EnvSettings {
    fn default() -> Self {
        Self {
            clear_color: Color::from_srgb_u8(14, 14, 24, 255),
            ambient: Color::WHITE,
            exposure: 1.0,
            tonemap: TonemapMode::Aces,
            bloom: true,
            bloom_intensity: 0.6,
            bloom_threshold: 1.0,
            bloom_radius: 1.0,
            vignette: 0.25,
            chromatic_aberration: 0.0,
            film_grain: 0.0,
            scanlines: 0.0,
            crt_curvature: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            tint: Color::WHITE,
            pixelate: 1.0,
        }
    }
}

/// 2D camera: world point at the screen center, zoom (pixels per unit) and rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View2D {
    pub center: Vec2,
    pub zoom: f32,
    pub rotation: f32,
}

impl Default for View2D {
    fn default() -> Self {
        Self { center: Vec2::ZERO, zoom: 1.0, rotation: 0.0 }
    }
}

impl View2D {
    /// World → screen pixels (origin top-left) for a viewport of `size`.
    pub fn world_to_screen(&self, size: Vec2) -> Affine2 {
        Affine2::from_translation(size * 0.5)
            * Affine2::from_scale(Vec2::splat(self.zoom))
            * Affine2::from_angle(-self.rotation)
            * Affine2::from_translation(-self.center)
    }

    pub fn screen_to_world(&self, size: Vec2, p: Vec2) -> Vec2 {
        self.world_to_screen(size).inverse().transform_point2(p)
    }
}

/// Everything needed to render one frame.
#[derive(Clone, Debug)]
pub struct DrawList {
    pub view: View2D,
    pub world: Vec<DrawCmd>,
    /// Screen-space (HUD) commands, in pixels; drawn after post-processing.
    pub screen: Vec<DrawCmd>,
    pub lights: Vec<Light>,
    pub env: EnvSettings,
    /// Full-screen flash overlay (alpha = strength).
    pub flash: Color,
    /// Seconds, drives grain/flicker animation.
    pub time: f32,
}

impl Default for DrawList {
    fn default() -> Self {
        Self {
            view: View2D::default(),
            world: Vec::new(),
            screen: Vec::new(),
            lights: Vec::new(),
            env: EnvSettings::default(),
            flash: Color::TRANSPARENT,
            time: 0.0,
        }
    }
}

impl DrawList {
    pub fn clear(&mut self) {
        self.world.clear();
        self.screen.clear();
        self.lights.clear();
        self.flash = Color::TRANSPARENT;
    }

    pub fn quad(&mut self, z: i32, q: QuadCmd) {
        self.world.push(DrawCmd { z, prim: Prim::Quad(q) });
    }

    pub fn text(&mut self, z: i32, t: TextCmd) {
        self.world.push(DrawCmd { z, prim: Prim::Text(t) });
    }
}
