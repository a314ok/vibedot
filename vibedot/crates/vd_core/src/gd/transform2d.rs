//! Port of Godot's `core/math/transform_2d.{h,cpp}`: a 2x3 matrix stored as
//! three columns `x`, `y` (basis) and `origin`.

use std::fmt;
use std::ops::{Mul, MulAssign};

use serde::{Deserialize, Serialize};

use super::math_funcs::{self as math, PI, Real};
use super::rect2::Rect2;
use super::vector2::{Size2, Vector2};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform2D {
    /// `[x, y, origin]` like Godot's `columns`.
    pub columns: [Vector2; 3],
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    pub const IDENTITY: Transform2D = Transform2D::from_columns(Vector2::new(1.0, 0.0), Vector2::new(0.0, 1.0), Vector2::ZERO);
    pub const FLIP_X: Transform2D = Transform2D::from_columns(Vector2::new(-1.0, 0.0), Vector2::new(0.0, 1.0), Vector2::ZERO);
    pub const FLIP_Y: Transform2D = Transform2D::from_columns(Vector2::new(1.0, 0.0), Vector2::new(0.0, -1.0), Vector2::ZERO);

    pub const fn from_columns(x: Vector2, y: Vector2, origin: Vector2) -> Self {
        Self { columns: [x, y, origin] }
    }

    /// `Transform2D(xx, xy, yx, yy, ox, oy)`.
    pub const fn new(xx: Real, xy: Real, yx: Real, yy: Real, ox: Real, oy: Real) -> Self {
        Self::from_columns(Vector2::new(xx, xy), Vector2::new(yx, yy), Vector2::new(ox, oy))
    }

    /// `Transform2D(rotation, position)`.
    pub fn from_rotation(rot: Real, pos: Vector2) -> Self {
        let (sr, cr) = rot.sin_cos();
        Self::from_columns(Vector2::new(cr, sr), Vector2::new(-sr, cr), pos)
    }

    /// `Transform2D(rotation, scale, skew, position)`.
    pub fn from_rotation_scale_skew(rot: Real, scale: Size2, skew: Real, pos: Vector2) -> Self {
        let mut t = Self::IDENTITY;
        t.columns[0].x = rot.cos() * scale.x;
        t.columns[1].y = (rot + skew).cos() * scale.y;
        t.columns[1].x = -(rot + skew).sin() * scale.y;
        t.columns[0].y = rot.sin() * scale.x;
        t.columns[2] = pos;
        t
    }

    #[inline]
    fn tdotx(&self, v: Vector2) -> Real {
        self.columns[0].x * v.x + self.columns[1].x * v.y
    }

    #[inline]
    fn tdoty(&self, v: Vector2) -> Real {
        self.columns[0].y * v.x + self.columns[1].y * v.y
    }

    pub fn get_origin(&self) -> Vector2 {
        self.columns[2]
    }

    pub fn set_origin(&mut self, o: Vector2) {
        self.columns[2] = o;
    }

    pub fn determinant(&self) -> Real {
        self.columns[0].x * self.columns[1].y - self.columns[0].y * self.columns[1].x
    }

    /// Assumes a rotation-only basis (like Godot's `inverse()`).
    pub fn inverse(&self) -> Self {
        let mut inv = *self;
        let tmp = inv.columns[0].y;
        inv.columns[0].y = inv.columns[1].x;
        inv.columns[1].x = tmp;
        inv.columns[2] = inv.basis_xform(-inv.columns[2]);
        inv
    }

    pub fn affine_inverse(&self) -> Self {
        let det = self.determinant();
        if cfg!(debug_assertions) && det == 0.0 {
            log::error!("Condition \"det == 0\" is true.");
            return *self;
        }
        let idet = 1.0 / det;
        let mut inv = *self;
        let tmp = inv.columns[0].x;
        inv.columns[0].x = inv.columns[1].y;
        inv.columns[1].y = tmp;
        inv.columns[0] *= Vector2::new(idet, -idet);
        inv.columns[1] *= Vector2::new(-idet, idet);
        inv.columns[2] = inv.basis_xform(-inv.columns[2]);
        inv
    }

    pub fn get_rotation(&self) -> Real {
        self.columns[0].y.atan2(self.columns[0].x)
    }

    pub fn set_rotation(&mut self, rot: Real) {
        let scale = self.get_scale();
        let (sr, cr) = rot.sin_cos();
        self.columns[0] = Vector2::new(cr, sr);
        self.columns[1] = Vector2::new(-sr, cr);
        self.set_scale(scale);
    }

    pub fn get_skew(&self) -> Real {
        let det = self.determinant();
        (self.columns[0].normalized().dot(self.columns[1].normalized() * math::sign(det))).acos() - PI as Real * 0.5
    }

    pub fn set_skew(&mut self, angle: Real) {
        let det = self.determinant();
        self.columns[1] =
            self.columns[0].rotated(PI as Real * 0.5 + angle).normalized() * math::sign(det) * self.columns[1].length();
    }

    pub fn get_scale(&self) -> Size2 {
        let det_sign = math::sign(self.determinant());
        Vector2::new(self.columns[0].length(), det_sign * self.columns[1].length())
    }

    pub fn set_scale(&mut self, scale: Size2) {
        self.columns[0].normalize();
        self.columns[1].normalize();
        self.columns[0] *= scale.x;
        self.columns[1] *= scale.y;
    }

    pub fn basis_xform(&self, v: Vector2) -> Vector2 {
        Vector2::new(self.tdotx(v), self.tdoty(v))
    }

    pub fn basis_xform_inv(&self, v: Vector2) -> Vector2 {
        Vector2::new(self.columns[0].dot(v), self.columns[1].dot(v))
    }

    pub fn xform(&self, v: Vector2) -> Vector2 {
        Vector2::new(self.tdotx(v), self.tdoty(v)) + self.columns[2]
    }

    pub fn xform_inv(&self, v: Vector2) -> Vector2 {
        let v = v - self.columns[2];
        Vector2::new(self.columns[0].dot(v), self.columns[1].dot(v))
    }

    pub fn xform_rect(&self, r: &Rect2) -> Rect2 {
        let x = self.columns[0] * r.size.x;
        let y = self.columns[1] * r.size.y;
        let pos = self.xform(r.position);
        let mut out = Rect2::from_pos_size(pos, Vector2::ZERO);
        out.expand_to(pos + x);
        out.expand_to(pos + y);
        out.expand_to(pos + x + y);
        out
    }

    pub fn xform_inv_rect(&self, r: &Rect2) -> Rect2 {
        let ends = [
            self.xform_inv(r.position),
            self.xform_inv(Vector2::new(r.position.x, r.position.y + r.size.y)),
            self.xform_inv(Vector2::new(r.position.x + r.size.x, r.position.y + r.size.y)),
            self.xform_inv(Vector2::new(r.position.x + r.size.x, r.position.y)),
        ];
        let mut out = Rect2::from_pos_size(ends[0], Vector2::ZERO);
        for e in &ends[1..] {
            out.expand_to(*e);
        }
        out
    }

    /// Equivalent to left multiplication by a scale.
    pub fn scaled(&self, scale: Size2) -> Self {
        let mut c = *self;
        c.columns[0].x *= scale.x;
        c.columns[0].y *= scale.y;
        c.columns[1].x *= scale.x;
        c.columns[1].y *= scale.y;
        c.columns[2] *= scale;
        c
    }

    /// Equivalent to right multiplication by a scale.
    pub fn scaled_local(&self, scale: Size2) -> Self {
        Self::from_columns(self.columns[0] * scale.x, self.columns[1] * scale.y, self.columns[2])
    }

    pub fn translated(&self, offset: Vector2) -> Self {
        Self::from_columns(self.columns[0], self.columns[1], self.columns[2] + offset)
    }

    pub fn translated_local(&self, offset: Vector2) -> Self {
        Self::from_columns(self.columns[0], self.columns[1], self.columns[2] + self.basis_xform(offset))
    }

    pub fn untranslated(&self) -> Self {
        Self::from_columns(self.columns[0], self.columns[1], Vector2::ZERO)
    }

    pub fn rotated(&self, angle: Real) -> Self {
        Self::from_rotation(angle, Vector2::ZERO) * *self
    }

    pub fn rotated_local(&self, angle: Real) -> Self {
        *self * Self::from_rotation(angle, Vector2::ZERO)
    }

    pub fn orthonormalized(&self) -> Self {
        let mut x = self.columns[0];
        let mut y = self.columns[1];
        x.normalize();
        y = y - x * x.dot(y);
        y.normalize();
        Self::from_columns(x, y, self.columns[2])
    }

    pub fn is_conformal(&self) -> bool {
        let c = &self.columns;
        (math::is_equal_approx(c[0].x, c[1].y) && math::is_equal_approx(c[0].y, -c[1].x))
            || (math::is_equal_approx(c[0].x, -c[1].y) && math::is_equal_approx(c[0].y, c[1].x))
    }

    pub fn is_equal_approx(&self, o: &Transform2D) -> bool {
        (0..3).all(|i| self.columns[i].is_equal_approx(o.columns[i]))
    }

    pub fn is_same(&self, o: &Transform2D) -> bool {
        (0..3).all(|i| self.columns[i].is_same(o.columns[i]))
    }

    pub fn is_finite(&self) -> bool {
        self.columns.iter().all(|c| c.is_finite())
    }

    pub fn looking_at(&self, target: Vector2) -> Self {
        let mut ret = Self::from_rotation(self.get_rotation(), self.get_origin());
        let target_position = self.affine_inverse().xform(target);
        let r = ret.get_rotation() + (target_position * self.get_scale()).angle();
        ret.set_rotation(r);
        ret
    }

    pub fn interpolate_with(&self, to: &Transform2D, weight: Real) -> Self {
        Self::from_rotation_scale_skew(
            math::lerp_angle(self.get_rotation(), to.get_rotation(), weight),
            self.get_scale().lerp(to.get_scale(), weight),
            math::lerp_angle(self.get_skew(), to.get_skew(), weight),
            self.get_origin().lerp(to.get_origin(), weight),
        )
    }

    pub fn to_affine(&self) -> glam::Affine2 {
        glam::Affine2::from_cols(self.columns[0].into(), self.columns[1].into(), self.columns[2].into())
    }

    pub fn from_affine(a: &glam::Affine2) -> Self {
        Self::from_columns(a.matrix2.x_axis.into(), a.matrix2.y_axis.into(), a.translation.into())
    }
}

impl MulAssign for Transform2D {
    fn mul_assign(&mut self, t: Transform2D) {
        self.columns[2] = self.xform(t.columns[2]);
        let x0 = self.tdotx(t.columns[0]);
        let x1 = self.tdoty(t.columns[0]);
        let y0 = self.tdotx(t.columns[1]);
        let y1 = self.tdoty(t.columns[1]);
        self.columns[0] = Vector2::new(x0, x1);
        self.columns[1] = Vector2::new(y0, y1);
    }
}

impl Mul for Transform2D {
    type Output = Transform2D;
    fn mul(self, t: Transform2D) -> Transform2D {
        let mut r = self;
        r *= t;
        r
    }
}

impl Mul<Vector2> for Transform2D {
    type Output = Vector2;
    fn mul(self, v: Vector2) -> Vector2 {
        self.xform(v)
    }
}

impl Mul<Real> for Transform2D {
    type Output = Transform2D;
    fn mul(self, s: Real) -> Transform2D {
        Self::from_columns(self.columns[0] * s, self.columns[1] * s, self.columns[2] * s)
    }
}

impl fmt::Display for Transform2D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[X: {}, Y: {}, O: {}]", self.columns[0], self.columns[1], self.columns[2])
    }
}
