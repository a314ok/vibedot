//! Port of Godot's `core/math/vector2.{h,cpp}`.

use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use super::math_funcs::{self as math, CMP_EPSILON, Real, UNIT_EPSILON};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X = 0,
    Y = 1,
}

/// 2D vector with Godot semantics (`real_t` = `f32`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vector2 {
    pub x: Real,
    pub y: Real,
}

pub type Size2 = Vector2;
pub type Point2 = Vector2;

impl Vector2 {
    pub const ZERO: Vector2 = Vector2::new(0.0, 0.0);
    pub const ONE: Vector2 = Vector2::new(1.0, 1.0);
    pub const INF: Vector2 = Vector2::new(Real::INFINITY, Real::INFINITY);
    pub const LEFT: Vector2 = Vector2::new(-1.0, 0.0);
    pub const RIGHT: Vector2 = Vector2::new(1.0, 0.0);
    /// Godot is y-down: UP is `(0, -1)`.
    pub const UP: Vector2 = Vector2::new(0.0, -1.0);
    pub const DOWN: Vector2 = Vector2::new(0.0, 1.0);

    #[inline]
    pub const fn new(x: Real, y: Real) -> Self {
        Self { x, y }
    }

    /// Godot's `Size2` accessors.
    #[inline]
    pub fn width(self) -> Real {
        self.x
    }
    #[inline]
    pub fn height(self) -> Real {
        self.y
    }

    pub fn min_axis_index(self) -> Axis {
        if self.x < self.y { Axis::X } else { Axis::Y }
    }

    pub fn max_axis_index(self) -> Axis {
        if self.x < self.y { Axis::Y } else { Axis::X }
    }

    pub fn angle(self) -> Real {
        self.y.atan2(self.x)
    }

    pub fn from_angle(angle: Real) -> Self {
        Self::new(angle.cos(), angle.sin())
    }

    pub fn length(self) -> Real {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn length_squared(self) -> Real {
        self.x * self.x + self.y * self.y
    }

    /// In place. Non-finite or zero vectors become `(0, 0)`.
    pub fn normalize(&mut self) {
        if !self.is_finite() {
            *self = Self::ZERO;
            return;
        }
        let l = self.length_squared();
        if l == 0.0 {
            *self = Self::ZERO;
        } else {
            let l = l.sqrt();
            self.x /= l;
            self.y /= l;
        }
    }

    pub fn normalized(self) -> Self {
        let mut v = self;
        v.normalize();
        v
    }

    pub fn is_normalized(self) -> bool {
        math::is_equal_approx_tol(self.length_squared(), 1.0, UNIT_EPSILON as Real)
    }

    pub fn distance_to(self, to: Vector2) -> Real {
        ((self.x - to.x) * (self.x - to.x) + (self.y - to.y) * (self.y - to.y)).sqrt()
    }

    pub fn distance_squared_to(self, to: Vector2) -> Real {
        (self.x - to.x) * (self.x - to.x) + (self.y - to.y) * (self.y - to.y)
    }

    pub fn angle_to(self, to: Vector2) -> Real {
        self.cross(to).atan2(self.dot(to))
    }

    pub fn angle_to_point(self, to: Vector2) -> Real {
        (to - self).angle()
    }

    pub fn dot(self, o: Vector2) -> Real {
        self.x * o.x + self.y * o.y
    }

    pub fn cross(self, o: Vector2) -> Real {
        self.x * o.y - self.y * o.x
    }

    pub fn sign(self) -> Self {
        Self::new(math::sign(self.x), math::sign(self.y))
    }

    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    pub fn inverse(self) -> Self {
        Self::new(1.0 / self.x, 1.0 / self.y)
    }

    pub fn min(self, o: Vector2) -> Self {
        Self::new(self.x.min(o.x), self.y.min(o.y))
    }

    pub fn minf(self, s: Real) -> Self {
        Self::new(self.x.min(s), self.y.min(s))
    }

    pub fn max(self, o: Vector2) -> Self {
        Self::new(self.x.max(o.x), self.y.max(o.y))
    }

    pub fn maxf(self, s: Real) -> Self {
        Self::new(self.x.max(s), self.y.max(s))
    }

    pub fn rotated(self, by: Real) -> Self {
        let (sine, cosi) = by.sin_cos();
        Self::new(self.x * cosi - self.y * sine, self.x * sine + self.y * cosi)
    }

    pub fn orthogonal(self) -> Self {
        Self::new(self.y, -self.x)
    }

    pub fn posmod(self, m: Real) -> Self {
        Self::new(math::fposmod(self.x, m), math::fposmod(self.y, m))
    }

    pub fn posmodv(self, m: Vector2) -> Self {
        Self::new(math::fposmod(self.x, m.x), math::fposmod(self.y, m.y))
    }

    pub fn project(self, to: Vector2) -> Self {
        to * (self.dot(to) / to.length_squared())
    }

    pub fn plane_project(self, d: Real, vec: Vector2) -> Self {
        vec - self * (self.dot(vec) - d)
    }

    pub fn clamp(self, min: Vector2, max: Vector2) -> Self {
        Self::new(self.x.max(min.x).min(max.x), self.y.max(min.y).min(max.y))
    }

    pub fn clampf(self, min: Real, max: Real) -> Self {
        Self::new(self.x.max(min).min(max), self.y.max(min).min(max))
    }

    pub fn snapped(self, step: Vector2) -> Self {
        Self::new(math::snapped(self.x, step.x), math::snapped(self.y, step.y))
    }

    pub fn snappedf(self, step: Real) -> Self {
        Self::new(math::snapped(self.x, step), math::snapped(self.y, step))
    }

    /// `limit_length()` with Godot's default length of 1.
    pub fn limit_length(self) -> Self {
        self.limit_length_to(1.0)
    }

    pub fn limit_length_to(self, len: Real) -> Self {
        let l = self.length();
        let mut v = self;
        if l > 0.0 && len < l {
            v /= l;
            v *= len;
        }
        v
    }

    pub fn move_toward(self, to: Vector2, delta: Real) -> Self {
        let vd = to - self;
        let len = vd.length();
        if len <= delta || len < CMP_EPSILON as Real { to } else { self + vd / len * delta }
    }

    /// Godot reports an error and returns `(0, 0)` for a non-normalized
    /// normal in debug builds (`MATH_CHECKS`); we do the same.
    fn check_normal(normal: Vector2, what: &str) -> bool {
        if cfg!(debug_assertions) && !normal.is_normalized() {
            log::error!("The normal Vector2 {normal} must be normalized ({what}).");
            return false;
        }
        true
    }

    pub fn slide(self, normal: Vector2) -> Self {
        if !Self::check_normal(normal, "slide") {
            return Self::ZERO;
        }
        self - normal * self.dot(normal)
    }

    pub fn bounce(self, normal: Vector2) -> Self {
        -self.reflect(normal)
    }

    pub fn reflect(self, normal: Vector2) -> Self {
        if !Self::check_normal(normal, "reflect") {
            return Self::ZERO;
        }
        normal * 2.0 * self.dot(normal) - self
    }

    pub fn lerp(self, to: Vector2, weight: Real) -> Self {
        Self::new(math::lerp(self.x, to.x, weight), math::lerp(self.y, to.y, weight))
    }

    pub fn slerp(self, to: Vector2, weight: Real) -> Self {
        let start_length_sq = self.length_squared();
        let end_length_sq = to.length_squared();
        if start_length_sq == 0.0 || end_length_sq == 0.0 {
            // Zero length vectors have no angle: fall back to lerp.
            return self.lerp(to, weight);
        }
        let start_length = start_length_sq.sqrt();
        let result_length = math::lerp(start_length, end_length_sq.sqrt(), weight);
        let angle = self.angle_to(to);
        self.rotated(angle * weight) * (result_length / start_length)
    }

    pub fn cubic_interpolate(self, b: Vector2, pre_a: Vector2, post_b: Vector2, weight: Real) -> Self {
        Self::new(
            math::cubic_interpolate(self.x, b.x, pre_a.x, post_b.x, weight),
            math::cubic_interpolate(self.y, b.y, pre_a.y, post_b.y, weight),
        )
    }

    pub fn cubic_interpolate_in_time(
        self,
        b: Vector2,
        pre_a: Vector2,
        post_b: Vector2,
        weight: Real,
        b_t: Real,
        pre_a_t: Real,
        post_b_t: Real,
    ) -> Self {
        Self::new(
            math::cubic_interpolate_in_time(self.x, b.x, pre_a.x, post_b.x, weight, b_t, pre_a_t, post_b_t),
            math::cubic_interpolate_in_time(self.y, b.y, pre_a.y, post_b.y, weight, b_t, pre_a_t, post_b_t),
        )
    }

    pub fn bezier_interpolate(self, c1: Vector2, c2: Vector2, end: Vector2, t: Real) -> Self {
        Self::new(
            math::bezier_interpolate(self.x, c1.x, c2.x, end.x, t),
            math::bezier_interpolate(self.y, c1.y, c2.y, end.y, t),
        )
    }

    pub fn bezier_derivative(self, c1: Vector2, c2: Vector2, end: Vector2, t: Real) -> Self {
        Self::new(
            math::bezier_derivative(self.x, c1.x, c2.x, end.x, t),
            math::bezier_derivative(self.y, c1.y, c2.y, end.y, t),
        )
    }

    pub fn direction_to(self, to: Vector2) -> Self {
        Self::new(to.x - self.x, to.y - self.y).normalized()
    }

    pub fn aspect(self) -> Real {
        self.x / self.y
    }

    pub fn is_equal_approx(self, o: Vector2) -> bool {
        math::is_equal_approx(self.x, o.x) && math::is_equal_approx(self.y, o.y)
    }

    pub fn is_same(self, o: Vector2) -> bool {
        math::is_same(self.x, o.x) && math::is_same(self.y, o.y)
    }

    pub fn is_zero_approx(self) -> bool {
        math::is_zero_approx(self.x) && math::is_zero_approx(self.y)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Godot's `Vector2i(Vector2)` cast (truncation toward zero).
    pub fn to_vector2i(self) -> (i32, i32) {
        (self.x as i32, self.y as i32)
    }
}

impl fmt::Display for Vector2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", math::string_num_real(self.x, true), math::string_num_real(self.y, true))
    }
}

impl Index<Axis> for Vector2 {
    type Output = Real;
    fn index(&self, a: Axis) -> &Real {
        match a {
            Axis::X => &self.x,
            Axis::Y => &self.y,
        }
    }
}

impl IndexMut<Axis> for Vector2 {
    fn index_mut(&mut self, a: Axis) -> &mut Real {
        match a {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
        }
    }
}

impl Index<usize> for Vector2 {
    type Output = Real;
    fn index(&self, i: usize) -> &Real {
        match i {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("Vector2 index {i} out of range"),
        }
    }
}

impl IndexMut<usize> for Vector2 {
    fn index_mut(&mut self, i: usize) -> &mut Real {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => panic!("Vector2 index {i} out of range"),
        }
    }
}

macro_rules! vec_ops {
    ($Tr:ident $f:ident $TrA:ident $fa:ident $op:tt) => {
        impl $Tr for Vector2 {
            type Output = Vector2;
            #[inline]
            fn $f(self, o: Vector2) -> Vector2 { Vector2::new(self.x $op o.x, self.y $op o.y) }
        }
        impl $Tr<Real> for Vector2 {
            type Output = Vector2;
            #[inline]
            fn $f(self, s: Real) -> Vector2 { Vector2::new(self.x $op s, self.y $op s) }
        }
        impl $TrA for Vector2 {
            #[inline]
            fn $fa(&mut self, o: Vector2) { self.x = self.x $op o.x; self.y = self.y $op o.y; }
        }
        impl $TrA<Real> for Vector2 {
            #[inline]
            fn $fa(&mut self, s: Real) { self.x = self.x $op s; self.y = self.y $op s; }
        }
    };
}
vec_ops!(Add add AddAssign add_assign +);
vec_ops!(Sub sub SubAssign sub_assign -);
vec_ops!(Mul mul MulAssign mul_assign *);
vec_ops!(Div div DivAssign div_assign /);

impl Mul<Vector2> for Real {
    type Output = Vector2;
    fn mul(self, v: Vector2) -> Vector2 {
        v * self
    }
}

impl Neg for Vector2 {
    type Output = Vector2;
    fn neg(self) -> Vector2 {
        Vector2::new(-self.x, -self.y)
    }
}

impl From<glam::Vec2> for Vector2 {
    fn from(v: glam::Vec2) -> Self {
        Self::new(v.x, v.y)
    }
}

impl From<Vector2> for glam::Vec2 {
    fn from(v: Vector2) -> Self {
        glam::Vec2::new(v.x, v.y)
    }
}
