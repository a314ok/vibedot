//! Port of Godot's `core/math/rect2.{h,cpp}`.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::math_funcs::{self as math, CMP_EPSILON, Real};
use super::transform2d::Transform2D;
use super::vector2::{Point2, Size2, Vector2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect2 {
    pub position: Point2,
    pub size: Size2,
}

impl Rect2 {
    pub const fn new(x: Real, y: Real, w: Real, h: Real) -> Self {
        Self { position: Vector2::new(x, y), size: Vector2::new(w, h) }
    }

    pub const fn from_pos_size(position: Point2, size: Size2) -> Self {
        Self { position, size }
    }

    fn warn_negative(&self, other: Option<&Rect2>) {
        if cfg!(debug_assertions)
            && (self.size.x < 0.0 || self.size.y < 0.0 || other.is_some_and(|o| o.size.x < 0.0 || o.size.y < 0.0))
        {
            log::error!("Rect2 size is negative, this is not supported. Use Rect2.abs() to get a Rect2 with a positive size.");
        }
    }

    pub fn get_area(&self) -> Real {
        self.size.x * self.size.y
    }

    pub fn get_center(&self) -> Vector2 {
        self.position + self.size * 0.5
    }

    pub fn get_end(&self) -> Vector2 {
        self.position + self.size
    }

    pub fn set_end(&mut self, end: Vector2) {
        self.size = end - self.position;
    }

    pub fn intersects(&self, r: &Rect2, include_borders: bool) -> bool {
        self.warn_negative(Some(r));
        let p = self.position;
        let s = self.size;
        if include_borders {
            !(p.x > r.position.x + r.size.x
                || p.x + s.x < r.position.x
                || p.y > r.position.y + r.size.y
                || p.y + s.y < r.position.y)
        } else {
            !(p.x >= r.position.x + r.size.x
                || p.x + s.x <= r.position.x
                || p.y >= r.position.y + r.size.y
                || p.y + s.y <= r.position.y)
        }
    }

    pub fn distance_to(&self, point: Vector2) -> Real {
        self.warn_negative(None);
        let mut dist: Real = 0.0;
        let mut inside = true;
        if point.x < self.position.x {
            dist = self.position.x - point.x;
            inside = false;
        }
        if point.y < self.position.y {
            let d = self.position.y - point.y;
            dist = if inside { d } else { dist.min(d) };
            inside = false;
        }
        if point.x >= self.position.x + self.size.x {
            let d = point.x - (self.position.x + self.size.x);
            dist = if inside { d } else { dist.min(d) };
            inside = false;
        }
        if point.y >= self.position.y + self.size.y {
            let d = point.y - (self.position.y + self.size.y);
            dist = if inside { d } else { dist.min(d) };
            inside = false;
        }
        if inside { 0.0 } else { dist }
    }

    pub fn encloses(&self, r: &Rect2) -> bool {
        self.warn_negative(Some(r));
        r.position.x >= self.position.x
            && r.position.y >= self.position.y
            && r.position.x + r.size.x <= self.position.x + self.size.x
            && r.position.y + r.size.y <= self.position.y + self.size.y
    }

    pub fn has_area(&self) -> bool {
        self.size.x > 0.0 && self.size.y > 0.0
    }

    pub fn intersection(&self, r: &Rect2) -> Rect2 {
        if !self.intersects(r, false) {
            return Rect2::default();
        }
        let position = r.position.max(self.position);
        let end = r.get_end().min(self.get_end());
        Rect2 { position, size: end - position }
    }

    pub fn merge(&self, r: &Rect2) -> Rect2 {
        self.warn_negative(Some(r));
        let position = r.position.min(self.position);
        let end = r.get_end().max(self.get_end());
        Rect2 { position, size: end - position }
    }

    pub fn has_point(&self, p: Point2) -> bool {
        self.warn_negative(None);
        !(p.x < self.position.x
            || p.y < self.position.y
            || p.x >= self.position.x + self.size.x
            || p.y >= self.position.y + self.size.y)
    }

    pub fn is_equal_approx(&self, r: &Rect2) -> bool {
        self.position.is_equal_approx(r.position) && self.size.is_equal_approx(r.size)
    }

    pub fn is_same(&self, r: &Rect2) -> bool {
        self.position.is_same(r.position) && self.size.is_same(r.size)
    }

    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.size.is_finite()
    }

    pub fn grow(&self, amount: Real) -> Rect2 {
        let mut g = *self;
        g.position.x -= amount;
        g.position.y -= amount;
        g.size.x += amount * 2.0;
        g.size.y += amount * 2.0;
        g
    }

    pub fn grow_side(&self, side: Side, amount: Real) -> Rect2 {
        self.grow_individual(
            if side == Side::Left { amount } else { 0.0 },
            if side == Side::Top { amount } else { 0.0 },
            if side == Side::Right { amount } else { 0.0 },
            if side == Side::Bottom { amount } else { 0.0 },
        )
    }

    pub fn grow_individual(&self, left: Real, top: Real, right: Real, bottom: Real) -> Rect2 {
        let mut g = *self;
        g.position.x -= left;
        g.position.y -= top;
        g.size.x += left + right;
        g.size.y += top + bottom;
        g
    }

    pub fn expand(&self, v: Vector2) -> Rect2 {
        let mut r = *self;
        r.expand_to(v);
        r
    }

    pub fn expand_to(&mut self, v: Vector2) {
        self.warn_negative(None);
        let mut begin = self.position;
        let mut end = self.position + self.size;
        if v.x < begin.x {
            begin.x = v.x;
        }
        if v.y < begin.y {
            begin.y = v.y;
        }
        if v.x > end.x {
            end.x = v.x;
        }
        if v.y > end.y {
            end.y = v.y;
        }
        self.position = begin;
        self.size = end - begin;
    }

    pub fn abs(&self) -> Rect2 {
        Rect2 { position: self.position + self.size.minf(0.0), size: self.size.abs() }
    }

    pub fn round(&self) -> Rect2 {
        Rect2 { position: self.position.round(), size: self.size.round() }
    }

    pub fn get_support(&self, direction: Vector2) -> Vector2 {
        let mut support = self.position;
        if direction.x > 0.0 {
            support.x += self.size.x;
        }
        if direction.y > 0.0 {
            support.y += self.size.y;
        }
        support
    }

    /// Returns `(hit position, normal)` if the segment intersects the rect.
    pub fn intersects_segment(&self, from: Point2, to: Point2) -> Option<(Point2, Vector2)> {
        self.warn_negative(None);
        let mut min: Real = 0.0;
        let mut max: Real = 1.0;
        let mut axis = 0;
        let mut sign: Real = 0.0;
        for i in 0..2 {
            let seg_from = from[i];
            let seg_to = to[i];
            let box_begin = self.position[i];
            let box_end = box_begin + self.size[i];
            let (cmin, cmax, csign);
            if seg_from < seg_to {
                if seg_from > box_end || seg_to < box_begin {
                    return None;
                }
                let length = seg_to - seg_from;
                cmin = if seg_from < box_begin { (box_begin - seg_from) / length } else { 0.0 };
                cmax = if seg_to > box_end { (box_end - seg_from) / length } else { 1.0 };
                csign = -1.0;
            } else {
                if seg_to > box_end || seg_from < box_begin {
                    return None;
                }
                let length = seg_to - seg_from;
                cmin = if seg_from > box_end { (box_end - seg_from) / length } else { 0.0 };
                cmax = if seg_to < box_begin { (box_begin - seg_from) / length } else { 1.0 };
                csign = 1.0;
            }
            if cmin > min {
                min = cmin;
                axis = i;
                sign = csign;
            }
            if cmax < max {
                max = cmax;
            }
            if max < min {
                return None;
            }
        }
        let rel = to - from;
        let mut normal = Vector2::ZERO;
        normal[axis] = sign;
        Some((from + rel * min, normal))
    }

    /// Separating-axis test between this rect and `rect` transformed by `xform`.
    pub fn intersects_transformed(&self, xform: &Transform2D, rect: &Rect2) -> bool {
        self.warn_negative(Some(rect));
        let xf_points = [
            xform.xform(rect.position),
            xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y)),
            xform.xform(Vector2::new(rect.position.x, rect.position.y + rect.size.y)),
            xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y + rect.size.y)),
        ];
        // Base rect axes first (cheaper).
        if !xf_points.iter().any(|p| p.y > self.position.y) {
            return false;
        }
        let low = self.position.y + self.size.y;
        if !xf_points.iter().any(|p| p.y < low) {
            return false;
        }
        if !xf_points.iter().any(|p| p.x > self.position.x) {
            return false;
        }
        let low = self.position.x + self.size.x;
        if !xf_points.iter().any(|p| p.x < low) {
            return false;
        }
        let own = [
            self.position,
            Vector2::new(self.position.x + self.size.x, self.position.y),
            Vector2::new(self.position.x, self.position.y + self.size.y),
            Vector2::new(self.position.x + self.size.x, self.position.y + self.size.y),
        ];
        for axis in [xform.columns[0], xform.columns[1]] {
            let (mina, maxa) = own.iter().fold((Real::INFINITY, Real::NEG_INFINITY), |(lo, hi), p| {
                let d = axis.dot(*p);
                (lo.min(d), hi.max(d))
            });
            let (minb, maxb) = xf_points.iter().fold((Real::INFINITY, Real::NEG_INFINITY), |(lo, hi), p| {
                let d = axis.dot(*p);
                (lo.min(d), hi.max(d))
            });
            if mina > maxb || minb > maxa {
                return false;
            }
        }
        true
    }

    /// Bounding box of the intersection between this rect and the polygon
    /// `xform * rect` (Sutherland-Hodgman clipping).
    pub fn intersection_transformed(&self, xform: &Transform2D, rect: &Rect2) -> Rect2 {
        self.warn_negative(Some(rect));
        let c = &xform.columns;
        if (math::is_zero_approx(c[0].y) && math::is_zero_approx(c[1].x))
            || (math::is_zero_approx(c[0].x) && math::is_zero_approx(c[1].y))
        {
            return self.intersection(&xform.xform_rect(rect));
        }
        if !self.intersects_transformed(xform, rect) {
            return Rect2::default();
        }
        let xf_points = [
            xform.xform(rect.position),
            xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y)),
            xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y + rect.size.y)),
            xform.xform(Vector2::new(rect.position.x, rect.position.y + rect.size.y)),
        ];
        let mut subject: Vec<Vector2> = xf_points.to_vec();
        let min = self.position;
        let max = self.position + self.size;
        let mut intersected = Vector2::ZERO;
        for edge in 0..4 {
            let axis = edge % 2;
            let another = 1 - axis;
            let is_min = edge < 2;
            intersected[axis] = if is_min { min[axis] } else { max[axis] };
            let inside = |p: &Vector2, lim: Real| if is_min { p[axis] >= lim } else { p[axis] <= lim };
            let mut output = Vec::with_capacity(8);
            let mut prev = *subject.last().expect("non-empty subject");
            let mut prev_in = inside(&prev, intersected[axis]);
            for &curr in &subject {
                let curr_in = inside(&curr, intersected[axis]);
                if prev_in != curr_in {
                    let t = (intersected[axis] - prev[axis]) / (curr[axis] - prev[axis]);
                    intersected[another] = prev[another] + (curr[another] - prev[another]) * t;
                    output.push(intersected);
                }
                if curr_in {
                    output.push(curr);
                }
                prev = curr;
                prev_in = curr_in;
            }
            subject = output;
            if subject.is_empty() {
                break;
            }
        }
        if !subject.is_empty() {
            return Rect2::from_points(&subject);
        }
        // Reverse containment: this rect may lie inside the transformed rect.
        let corners = [self.position, Vector2::new(max.x, min.y), max, Vector2::new(min.x, max.y)];
        let inside_points: Vec<Vector2> = corners
            .iter()
            .copied()
            .filter(|&corner| {
                let (mut has_pos, mut has_neg) = (false, false);
                for idx in 0..4 {
                    let v0 = xf_points[(idx + 1) % 4] - xf_points[idx];
                    let v1 = corner - xf_points[idx];
                    let cross = v0.cross(v1);
                    has_pos |= cross > CMP_EPSILON as Real;
                    has_neg |= cross < -(CMP_EPSILON as Real);
                    if has_pos && has_neg {
                        break;
                    }
                }
                !(has_pos && has_neg)
            })
            .collect();
        if inside_points.is_empty() { Rect2::default() } else { Rect2::from_points(&inside_points) }
    }

    pub fn from_points(points: &[Vector2]) -> Rect2 {
        let Some(&first) = points.first() else {
            log::error!("The number of points passed in is invalid.");
            return Rect2::default();
        };
        let mut position = first;
        let mut end = first;
        for &p in &points[1..] {
            if p.x < position.x {
                position.x = p.x;
            } else if p.x > end.x {
                end.x = p.x;
            }
            if p.y < position.y {
                position.y = p.y;
            } else if p.y > end.y {
                end.y = p.y;
            }
        }
        Rect2 { position, size: end - position }
    }
}

impl fmt::Display for Rect2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[P: {}, S: {}]", self.position, self.size)
    }
}
