//! Built-in node types. Each is a plain data struct; behaviour lives in the
//! engine systems (render, physics, particles, timers) that read them -
//! a data-oriented take on Godot's class hierarchy.

use vd_core::{Color, Vec2, vec2};

use crate::reflect::PropHint;
use crate::{node_kind, prop_enum};

const NONE: PropHint = PropHint::None;

prop_enum! {
    /// Geometric primitive drawn by [`Shape2D`] (rendered as an anti-aliased SDF).
    pub enum ShapeType { Rect, RoundedRect, Circle, Ring, Triangle } default Rect
}

prop_enum! {
    pub enum BodyType { Static, Character, Rigid, Area } default Character
}

prop_enum! {
    pub enum CollisionShape { Rect, Circle } default Rect
}

prop_enum! {
    pub enum TextAlign { Left, Center, Right } default Center
}

prop_enum! {
    pub enum Tonemap { Linear, Reinhard, Aces, AgX } default Aces
}

prop_enum! {
    pub enum BlendMode { Alpha, Additive } default Alpha
}

node_kind! {
    /// Textured quad (Godot `Sprite2D`). Without a texture it draws a colored rect.
    pub struct Sprite2D ["Sprite2D"] {
        texture: String = String::new() => PropHint::File(&["png"]),
        size: Vec2 = vec2(64.0, 64.0) => NONE,
        color: Color = Color::WHITE => PropHint::HdrColor,
        /// Values > 0 make the sprite self-lit and feed the bloom pass.
        emission: f32 = 0.0 => PropHint::Range { min: 0.0, max: 20.0, step: 0.1 },
        flip_h: bool = false => NONE,
        flip_v: bool = false => NONE,
        centered: bool = true => NONE,
        /// Sprite-sheet frames: hframes x vframes grid, current `frame`.
        hframes: u32 = 1 => PropHint::Range { min: 1.0, max: 64.0, step: 1.0 },
        vframes: u32 = 1 => PropHint::Range { min: 1.0, max: 64.0, step: 1.0 },
        frame: u32 = 0 => PropHint::Range { min: 0.0, max: 4095.0, step: 1.0 },
        /// Frames per second for automatic animation (0 = off).
        anim_fps: f32 = 0.0 => PropHint::Range { min: 0.0, max: 60.0, step: 1.0 },
        blend: BlendMode = BlendMode::Alpha => PropHint::Enum(BlendMode::NAMES),
    }
    runtime { anim_time: f32 }
}

node_kind! {
    /// Vector primitive with crisp, resolution-independent edges.
    pub struct Shape2D ["Shape2D"] {
        shape: ShapeType = ShapeType::Rect => PropHint::Enum(ShapeType::NAMES),
        size: Vec2 = vec2(64.0, 64.0) => NONE,
        corner_radius: f32 = 8.0 => PropHint::Range { min: 0.0, max: 256.0, step: 0.5 },
        /// Ring thickness / outline width.
        thickness: f32 = 6.0 => PropHint::Range { min: 0.0, max: 256.0, step: 0.5 },
        color: Color = Color::WHITE => PropHint::HdrColor,
        emission: f32 = 0.0 => PropHint::Range { min: 0.0, max: 20.0, step: 0.1 },
        blend: BlendMode = BlendMode::Alpha => PropHint::Enum(BlendMode::NAMES),
        /// Soft edge in pixels (0.0 = sharp but anti-aliased).
        softness: f32 = 0.0 => PropHint::Range { min: 0.0, max: 64.0, step: 0.5 },
    }
}

node_kind! {
    pub struct Label ["Label"] {
        text: String = "Label".to_owned() => PropHint::Multiline,
        font_size: f32 = 24.0 => PropHint::Range { min: 4.0, max: 256.0, step: 1.0 },
        color: Color = Color::WHITE => PropHint::HdrColor,
        emission: f32 = 0.0 => PropHint::Range { min: 0.0, max: 20.0, step: 0.1 },
        align: TextAlign = TextAlign::Center => PropHint::Enum(TextAlign::NAMES),
        /// Draw in screen space (HUD) instead of world space.
        screen_space: bool = false => NONE,
        monospace: bool = false => NONE,
    }
}

node_kind! {
    pub struct Camera2D ["Camera2D"] {
        current: bool = true => NONE,
        zoom: f32 = 1.0 => PropHint::Range { min: 0.05, max: 20.0, step: 0.05 },
        /// Follow smoothing speed (0 = instant).
        smoothing: f32 = 0.0 => PropHint::Range { min: 0.0, max: 30.0, step: 0.5 },
        /// Screen shake amplitude in pixels at trauma 1.0.
        shake_strength: f32 = 12.0 => PropHint::Range { min: 0.0, max: 100.0, step: 0.5 },
        /// How fast trauma decays per second.
        shake_decay: f32 = 1.5 => PropHint::Range { min: 0.0, max: 10.0, step: 0.1 },
    }
    runtime { trauma: f32, smoothed: Option<Vec2> }
}

node_kind! {
    /// Additive 2D light. Only affects the scene if `ambient` < 1 in the environment.
    pub struct PointLight2D ["PointLight2D"] {
        color: Color = Color::rgb(1.0, 0.85, 0.6) => PropHint::HdrColor,
        energy: f32 = 1.5 => PropHint::Range { min: 0.0, max: 16.0, step: 0.05 },
        radius: f32 = 256.0 => PropHint::Range { min: 1.0, max: 4096.0, step: 1.0 },
        /// Falloff exponent: 1 = linear, 2 = smooth, higher = tighter core.
        falloff: f32 = 2.0 => PropHint::Range { min: 0.25, max: 8.0, step: 0.05 },
        flicker: f32 = 0.0 => PropHint::Range { min: 0.0, max: 1.0, step: 0.01 },
    }
}

/// A single live particle (structure-of-arrays would be faster for SIMD; with
/// a few thousand particles AoS keeps the code simple and is cache friendly).
#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub age: f32,
    pub life: f32,
    pub spin: f32,
    pub angle: f32,
    pub size_jitter: f32,
}

node_kind! {
    /// CPU particle emitter (Godot `CPUParticles2D`).
    pub struct Particles2D ["Particles2D"] {
        emitting: bool = true => NONE,
        /// Particles per second.
        rate: f32 = 60.0 => PropHint::Range { min: 0.0, max: 5000.0, step: 1.0 },
        /// Emit `burst` particles at once when emission starts / on `burst()`.
        burst: u32 = 0 => PropHint::Range { min: 0.0, max: 5000.0, step: 1.0 },
        one_shot: bool = false => NONE,
        lifetime: f32 = 1.2 => PropHint::Range { min: 0.01, max: 30.0, step: 0.01 },
        lifetime_random: f32 = 0.3 => PropHint::Range { min: 0.0, max: 1.0, step: 0.01 },
        direction: f32 = -1.5708 => PropHint::Angle,
        spread: f32 = 0.6 => PropHint::Angle,
        speed_min: f32 = 60.0 => PropHint::Range { min: 0.0, max: 4000.0, step: 1.0 },
        speed_max: f32 = 140.0 => PropHint::Range { min: 0.0, max: 4000.0, step: 1.0 },
        gravity: Vec2 = vec2(0.0, 98.0) => NONE,
        damping: f32 = 0.5 => PropHint::Range { min: 0.0, max: 20.0, step: 0.05 },
        emit_radius: f32 = 0.0 => PropHint::Range { min: 0.0, max: 1024.0, step: 1.0 },
        size_start: f32 = 10.0 => PropHint::Range { min: 0.0, max: 512.0, step: 0.5 },
        size_end: f32 = 0.0 => PropHint::Range { min: 0.0, max: 512.0, step: 0.5 },
        color_start: Color = Color::rgb(1.0, 0.8, 0.3) => PropHint::HdrColor,
        color_end: Color = Color::rgba(1.0, 0.2, 0.1, 0.0) => PropHint::HdrColor,
        emission: f32 = 2.0 => PropHint::Range { min: 0.0, max: 20.0, step: 0.1 },
        shape: ShapeType = ShapeType::Circle => PropHint::Enum(ShapeType::NAMES),
        blend: BlendMode = BlendMode::Additive => PropHint::Enum(BlendMode::NAMES),
        /// Simulate in world space (trails stay behind when the emitter moves).
        world_space: bool = true => NONE,
        max_particles: u32 = 2000 => PropHint::Range { min: 1.0, max: 100000.0, step: 1.0 },
    }
    runtime { particles: Vec<Particle>, accumulator: f32, burst_pending: bool, started: bool }
}

node_kind! {
    /// Physics body (Godot `StaticBody2D` / `CharacterBody2D` / `RigidBody2D` / `Area2D`).
    pub struct Body2D ["Physics"] {
        body_type: BodyType = BodyType::Character => PropHint::Enum(BodyType::NAMES),
        shape: CollisionShape = CollisionShape::Rect => PropHint::Enum(CollisionShape::NAMES),
        size: Vec2 = vec2(64.0, 64.0) => NONE,
        radius: f32 = 32.0 => PropHint::Range { min: 0.5, max: 4096.0, step: 0.5 },
        velocity: Vec2 = Vec2::ZERO => NONE,
        gravity_scale: f32 = 0.0 => PropHint::Range { min: -10.0, max: 10.0, step: 0.05 },
        mass: f32 = 1.0 => PropHint::Range { min: 0.01, max: 1000.0, step: 0.01 },
        bounce: f32 = 0.0 => PropHint::Range { min: 0.0, max: 1.0, step: 0.01 },
        friction: f32 = 0.2 => PropHint::Range { min: 0.0, max: 1.0, step: 0.01 },
        /// Which layers this body is on (bit mask).
        layer: u32 = 1 => NONE,
        /// Which layers this body collides with / detects (bit mask).
        mask: u32 = 1 => NONE,
        /// Character bodies: keep inside the camera's world bounds.
        clamp_to_view: bool = false => NONE,
    }
    runtime { on_floor: bool }
}

node_kind! {
    pub struct Timer ["Timer"] {
        wait_time: f32 = 1.0 => PropHint::Range { min: 0.001, max: 3600.0, step: 0.01 },
        one_shot: bool = false => NONE,
        autostart: bool = true => NONE,
    }
    runtime { time_left: f32, running: bool, initialized: bool }
}

node_kind! {
    /// Scene-wide rendering settings (Godot `WorldEnvironment` + `Environment`).
    pub struct WorldEnvironment ["Environment"] {
        clear_color: Color = Color::from_srgb_u8(14, 14, 24, 255) => PropHint::HdrColor,
        /// 1.0 = fully lit (lights have no visible effect); lower it for moody scenes.
        ambient: f32 = 1.0 => PropHint::Range { min: 0.0, max: 2.0, step: 0.01 },
        ambient_color: Color = Color::WHITE => PropHint::HdrColor,
        exposure: f32 = 1.0 => PropHint::Range { min: 0.05, max: 8.0, step: 0.01 },
        tonemap: Tonemap = Tonemap::Aces => PropHint::Enum(Tonemap::NAMES),
        bloom: bool = true => NONE,
        bloom_intensity: f32 = 0.6 => PropHint::Range { min: 0.0, max: 4.0, step: 0.01 },
        bloom_threshold: f32 = 1.0 => PropHint::Range { min: 0.0, max: 8.0, step: 0.01 },
        bloom_radius: f32 = 1.0 => PropHint::Range { min: 0.1, max: 3.0, step: 0.01 },
        vignette: f32 = 0.25 => PropHint::Range { min: 0.0, max: 1.5, step: 0.01 },
        chromatic_aberration: f32 = 0.0 => PropHint::Range { min: 0.0, max: 10.0, step: 0.05 },
        film_grain: f32 = 0.0 => PropHint::Range { min: 0.0, max: 0.5, step: 0.005 },
        scanlines: f32 = 0.0 => PropHint::Range { min: 0.0, max: 1.0, step: 0.01 },
        crt_curvature: f32 = 0.0 => PropHint::Range { min: 0.0, max: 0.5, step: 0.005 },
        saturation: f32 = 1.0 => PropHint::Range { min: 0.0, max: 2.0, step: 0.01 },
        contrast: f32 = 1.0 => PropHint::Range { min: 0.5, max: 2.0, step: 0.01 },
        tint: Color = Color::WHITE => NONE,
        /// Render at 1/pixelate resolution for a retro look (1 = off).
        pixelate: f32 = 1.0 => PropHint::Range { min: 1.0, max: 16.0, step: 1.0 },
    }
}

node_kind! {
    /// Groups children; has no visual of its own (Godot `Node2D`).
    pub struct Node2D ["Node2D"] {}
}

node_kind! {
    /// Plain node without a transform (Godot `Node`).
    pub struct PlainNode ["Node"] {}
}
