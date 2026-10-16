//! 2D math types layered on top of `glam` (SIMD-friendly, `#[repr(C)]`).

pub use glam::{Affine2, Mat2, Mat3, Mat4, Vec2, Vec3, Vec4, vec2, vec3, vec4};
use serde::{Deserialize, Serialize};

/// Decomposed 2D transform, the editable form of Godot's `Transform2D`.
///
/// Nodes store this (it is what the inspector edits); the renderer and physics
/// consume the composed [`Affine2`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform2D {
    pub position: Vec2,
    /// Radians, counter-clockwise in a y-down world (i.e. visually clockwise).
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    pub const IDENTITY: Self = Self { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::ONE };

    pub fn from_position(position: Vec2) -> Self {
        Self { position, ..Self::IDENTITY }
    }

    #[inline]
    pub fn to_affine(&self) -> Affine2 {
        Affine2::from_scale_angle_translation(self.scale, self.rotation, self.position)
    }

    /// Decompose an affine (assumes no shear, which holds for scene transforms).
    pub fn from_affine(a: &Affine2) -> Self {
        let (scale, rotation, position) = a.to_scale_angle_translation();
        Self { position, rotation, scale }
    }
}

/// Axis-aligned rectangle (Godot's `Rect2`), defined by min corner + size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect2 {
    pub min: Vec2,
    pub size: Vec2,
}

impl Rect2 {
    pub const fn new(min: Vec2, size: Vec2) -> Self {
        Self { min, size }
    }

    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        Self { min: center - size * 0.5, size }
    }

    pub fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self { min, size: max - min }
    }

    #[inline]
    pub fn max(&self) -> Vec2 {
        self.min + self.size
    }

    #[inline]
    pub fn center(&self) -> Vec2 {
        self.min + self.size * 0.5
    }

    #[inline]
    pub fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmplt(self.max()).all()
    }

    #[inline]
    pub fn intersects(&self, o: &Rect2) -> bool {
        self.min.cmplt(o.max()).all() && o.min.cmplt(self.max()).all()
    }

    pub fn union(&self, o: &Rect2) -> Rect2 {
        Rect2::from_min_max(self.min.min(o.min), self.max().max(o.max()))
    }

    pub fn grow(&self, by: f32) -> Rect2 {
        Rect2 { min: self.min - Vec2::splat(by), size: self.size + Vec2::splat(by * 2.0) }
    }

    /// Bounding box of this rect after an affine transform.
    pub fn transformed(&self, a: &Affine2) -> Rect2 {
        let corners = [self.min, vec2(self.max().x, self.min.y), self.max(), vec2(self.min.x, self.max().y)].map(|c| a.transform_point2(c));
        let mut lo = corners[0];
        let mut hi = corners[0];
        for c in &corners[1..] {
            lo = lo.min(*c);
            hi = hi.max(*c);
        }
        Rect2::from_min_max(lo, hi)
    }
}

/// Frame-rate independent exponential smoothing factor (`1 - e^(-speed*dt)`).
#[inline]
pub fn damp_factor(speed: f32, dt: f32) -> f32 {
    1.0 - (-speed * dt).exp()
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Shortest-path angle interpolation.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let diff = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + diff * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_roundtrip() {
        let t = Transform2D { position: vec2(10.0, -4.0), rotation: 0.7, scale: vec2(2.0, 0.5) };
        let back = Transform2D::from_affine(&t.to_affine());
        assert!((back.position - t.position).length() < 1e-4);
        assert!((back.rotation - t.rotation).abs() < 1e-4);
        assert!((back.scale - t.scale).length() < 1e-4);
    }

    #[test]
    fn rect_ops() {
        let r = Rect2::from_center_size(Vec2::ZERO, vec2(2.0, 2.0));
        assert!(r.contains(vec2(0.5, -0.5)));
        assert!(!r.contains(vec2(1.5, 0.0)));
        assert!(r.intersects(&Rect2::new(vec2(0.9, 0.9), vec2(1.0, 1.0))));
        assert!(!r.intersects(&Rect2::new(vec2(1.0, 1.0), vec2(1.0, 1.0))));
    }

    #[test]
    fn angle_lerp_takes_short_path() {
        let a = lerp_angle(3.0, -3.0, 0.5);
        assert!(a.abs() > 3.0, "should wrap through PI, got {a}");
    }
}
