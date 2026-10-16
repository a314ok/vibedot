//! # vd_physics
//!
//! A compact 2D physics server (Godot's `PhysicsServer2D`, simplified):
//! axis-aligned boxes and circles, sweep-and-prune broadphase, iterative
//! position-based contact resolution, character "move and slide", rigid
//! bounce/friction, overlap-only areas and enter/exit contact events.
//!
//! The world is rebuilt from the scene every tick (bodies are plain data);
//! only the persistent contact set lives here, which keeps ownership simple
//! and avoids handle synchronisation bugs between the scene and physics.
#![forbid(unsafe_code)]

use std::collections::HashSet;

use vd_core::{Rect2, Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Static,
    /// Moved by its velocity, slides along obstacles, unaffected by pushes.
    Character,
    Rigid,
    /// Detects overlaps only.
    Area,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Rect { half: Vec2 },
    Circle { radius: f32 },
}

impl Shape {
    pub fn aabb(&self, pos: Vec2) -> Rect2 {
        match *self {
            Shape::Rect { half } => Rect2::from_center_size(pos, half * 2.0),
            Shape::Circle { radius } => Rect2::from_center_size(pos, Vec2::splat(radius * 2.0)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Body {
    /// Opaque owner id (the engine stores the scene node handle bits).
    pub key: u64,
    pub kind: BodyKind,
    pub shape: Shape,
    pub pos: Vec2,
    pub vel: Vec2,
    pub mass: f32,
    pub bounce: f32,
    pub friction: f32,
    pub gravity_scale: f32,
    pub layer: u32,
    pub mask: u32,
    /// Output: touching ground this step.
    pub on_floor: bool,
}

impl Body {
    pub fn new(key: u64, kind: BodyKind, shape: Shape, pos: Vec2) -> Self {
        Self {
            key,
            kind,
            shape,
            pos,
            vel: Vec2::ZERO,
            mass: 1.0,
            bounce: 0.0,
            friction: 0.2,
            gravity_scale: 0.0,
            layer: 1,
            mask: 1,
            on_floor: false,
        }
    }

    fn inv_mass(&self) -> f32 {
        match self.kind {
            BodyKind::Rigid => 1.0 / self.mass.max(1e-4),
            _ => 0.0,
        }
    }

    fn is_solid(&self) -> bool {
        self.kind != BodyKind::Area
    }

    fn moves(&self) -> bool {
        matches!(self.kind, BodyKind::Character | BodyKind::Rigid)
    }
}

/// Contact pair, ordered so that `a < b`.
pub type Pair = (u64, u64);

#[derive(Clone, Debug, Default)]
pub struct StepEvents {
    pub entered: Vec<Pair>,
    pub exited: Vec<Pair>,
}

#[derive(Clone, Debug)]
pub struct World {
    pub gravity: Vec2,
    pub bodies: Vec<Body>,
    pub iterations: u32,
    contacts: HashSet<Pair>,
}

impl Default for World {
    fn default() -> Self {
        Self { gravity: vec2(0.0, 980.0), bodies: Vec::new(), iterations: 6, contacts: HashSet::new() }
    }
}

/// Minimum translation vector pushing `a` out of `b` (normal points from b to a).
pub fn penetration(a_shape: &Shape, a_pos: Vec2, b_shape: &Shape, b_pos: Vec2) -> Option<(Vec2, f32)> {
    match (*a_shape, *b_shape) {
        (Shape::Rect { half: ha }, Shape::Rect { half: hb }) => {
            let d = a_pos - b_pos;
            let overlap = ha + hb - d.abs();
            if overlap.x <= 0.0 || overlap.y <= 0.0 {
                return None;
            }
            if overlap.x < overlap.y {
                Some((vec2(if d.x < 0.0 { -1.0 } else { 1.0 }, 0.0), overlap.x))
            } else {
                Some((vec2(0.0, if d.y < 0.0 { -1.0 } else { 1.0 }), overlap.y))
            }
        }
        (Shape::Circle { radius: ra }, Shape::Circle { radius: rb }) => {
            let d = a_pos - b_pos;
            let dist2 = d.length_squared();
            let r = ra + rb;
            if dist2 >= r * r {
                return None;
            }
            let dist = dist2.sqrt();
            let n = if dist > 1e-6 { d / dist } else { vec2(0.0, -1.0) };
            Some((n, r - dist))
        }
        (Shape::Circle { radius }, Shape::Rect { half }) => circle_rect(a_pos, radius, b_pos, half),
        (Shape::Rect { half }, Shape::Circle { radius }) => {
            circle_rect(b_pos, radius, a_pos, half).map(|(n, d)| (-n, d))
        }
    }
}

/// Circle at `c` vs box at `b`; normal points from the box to the circle.
fn circle_rect(c: Vec2, r: f32, b: Vec2, half: Vec2) -> Option<(Vec2, f32)> {
    let local = c - b;
    let closest = local.clamp(-half, half);
    let inside = closest == local;
    if inside {
        // Center inside the box: push out along the axis of least depth.
        let depth = half - local.abs();
        return Some(if depth.x < depth.y {
            (vec2(local.x.signum(), 0.0), depth.x + r)
        } else {
            (vec2(0.0, if local.y < 0.0 { -1.0 } else { 1.0 }), depth.y + r)
        });
    }
    let d = local - closest;
    let dist2 = d.length_squared();
    if dist2 >= r * r {
        return None;
    }
    let dist = dist2.sqrt();
    Some((d / dist.max(1e-6), r - dist))
}

fn interacts(a: &Body, b: &Body) -> bool {
    (a.mask & b.layer) != 0 || (b.mask & a.layer) != 0
}

impl World {
    pub fn new(gravity: Vec2) -> Self {
        Self { gravity, ..Default::default() }
    }

    pub fn clear_bodies(&mut self) {
        self.bodies.clear();
    }

    /// Sweep-and-prune broadphase: candidate pairs whose AABBs overlap
    /// (expanded by `skin`). O(n log n + k).
    fn broadphase(&self, skin: f32) -> Vec<(usize, usize)> {
        let boxes: Vec<Rect2> = self.bodies.iter().map(|b| b.shape.aabb(b.pos).grow(skin)).collect();
        let mut order: Vec<usize> = (0..self.bodies.len()).collect();
        order.sort_unstable_by(|&i, &j| boxes[i].min.x.total_cmp(&boxes[j].min.x));
        let mut pairs = Vec::new();
        let mut active: Vec<usize> = Vec::new();
        for &i in &order {
            let min_x = boxes[i].min.x;
            active.retain(|&j| boxes[j].max().x > min_x);
            for &j in &active {
                if boxes[i].intersects(&boxes[j]) {
                    let (a, b) = if i < j { (i, j) } else { (j, i) };
                    // Two static/area-only bodies never need resolving or events
                    // unless one is an area.
                    let (ba, bb) = (&self.bodies[a], &self.bodies[b]);
                    let static_pair = !ba.moves() && !bb.moves() && ba.kind != BodyKind::Area && bb.kind != BodyKind::Area;
                    if !static_pair && interacts(ba, bb) {
                        pairs.push((a, b));
                    }
                }
            }
            active.push(i);
        }
        pairs
    }

    /// Advances the simulation by `dt` seconds and reports contact changes.
    pub fn step(&mut self, dt: f32) -> StepEvents {
        // 1. Integrate velocities and positions.
        for b in &mut self.bodies {
            b.on_floor = false;
            if b.moves() {
                b.vel += self.gravity * b.gravity_scale * dt;
                b.pos += b.vel * dt;
            }
        }

        // 2. Resolve penetrations iteratively.
        for _ in 0..self.iterations {
            let pairs = self.broadphase(0.0);
            let mut any = false;
            for (i, j) in pairs {
                let (bi, bj) = (&self.bodies[i], &self.bodies[j]);
                if !bi.is_solid() || !bj.is_solid() || (!bi.moves() && !bj.moves()) {
                    continue;
                }
                let Some((n, depth)) = penetration(&bi.shape, bi.pos, &bj.shape, bj.pos) else { continue };
                if depth <= 1e-4 {
                    continue;
                }
                any = true;
                self.resolve(i, j, n, depth);
            }
            if !any {
                break;
            }
        }

        // 3. Contact events (touching within a small skin counts as contact).
        let skin = 0.75;
        let mut now = HashSet::new();
        for (i, j) in self.broadphase(skin) {
            let (bi, bj) = (&self.bodies[i], &self.bodies[j]);
            let grown = |s: &Shape| match *s {
                Shape::Rect { half } => Shape::Rect { half: half + Vec2::splat(skin) },
                Shape::Circle { radius } => Shape::Circle { radius: radius + skin },
            };
            if penetration(&grown(&bi.shape), bi.pos, &bj.shape, bj.pos).is_some() {
                let pair = if bi.key < bj.key { (bi.key, bj.key) } else { (bj.key, bi.key) };
                now.insert(pair);
            }
        }
        let mut ev = StepEvents::default();
        ev.entered.extend(now.difference(&self.contacts).copied());
        ev.exited.extend(self.contacts.difference(&now).copied());
        ev.entered.sort_unstable();
        ev.exited.sort_unstable();
        self.contacts = now;
        ev
    }

    /// Pushes bodies `i` and `j` apart along `n` (pointing from j to i).
    fn resolve(&mut self, i: usize, j: usize, n: Vec2, depth: f32) {
        let (wi, wj) = {
            let (bi, bj) = (&self.bodies[i], &self.bodies[j]);
            // Characters behave as infinitely heavy against rigid bodies, but
            // are pushed out of statics and other characters.
            let w = |me: &Body, other: &Body| -> f32 {
                match (me.kind, other.kind) {
                    (BodyKind::Static, _) => 0.0,
                    (BodyKind::Character, BodyKind::Rigid) => 0.0,
                    (BodyKind::Character, _) => 1.0,
                    (BodyKind::Rigid, BodyKind::Character) => 1.0,
                    (BodyKind::Rigid, _) => me.inv_mass(),
                    (BodyKind::Area, _) => 0.0,
                }
            };
            let (a, b) = (w(bi, bj), w(bj, bi));
            let total = a + b;
            if total <= 0.0 {
                return;
            }
            (a / total, b / total)
        };
        self.bodies[i].pos += n * depth * wi;
        self.bodies[j].pos -= n * depth * wj;

        let rel = self.bodies[i].vel - self.bodies[j].vel;
        let vn = rel.dot(n);
        // Floor detection: y-down world, so "up" normal is (0,-1).
        if n.y < -0.6 {
            self.bodies[i].on_floor = true;
        }
        if n.y > 0.6 {
            self.bodies[j].on_floor = true;
        }
        if vn >= 0.0 {
            return; // already separating
        }
        let bounce = self.bodies[i].bounce.max(self.bodies[j].bounce);
        let friction = (self.bodies[i].friction * self.bodies[j].friction).sqrt();
        let tangent = rel - n * vn;
        for (idx, w, sign) in [(i, wi, 1.0f32), (j, wj, -1.0f32)] {
            if w <= 0.0 {
                continue;
            }
            let b = &mut self.bodies[idx];
            let restitution = if b.kind == BodyKind::Character { 0.0 } else { bounce };
            // Remove (and optionally reflect) the approaching normal velocity.
            b.vel -= n * vn * (1.0 + restitution) * w * sign;
            if b.kind == BodyKind::Rigid {
                b.vel -= tangent * friction.min(1.0) * 0.5 * w * sign;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> Body {
        Body::new(1, BodyKind::Static, Shape::Rect { half: vec2(500.0, 20.0) }, vec2(0.0, 100.0))
    }

    #[test]
    fn rigid_ball_lands_on_floor() {
        let mut w = World::default();
        w.bodies.push(floor());
        let mut ball = Body::new(2, BodyKind::Rigid, Shape::Circle { radius: 10.0 }, vec2(0.0, 0.0));
        ball.gravity_scale = 1.0;
        w.bodies.push(ball);
        let mut entered = false;
        for _ in 0..120 {
            let ev = w.step(1.0 / 60.0);
            entered |= ev.entered.contains(&(1, 2));
        }
        let b = &w.bodies[1];
        assert!(entered, "contact event should fire");
        assert!((b.pos.y - 70.0).abs() < 1.5, "ball rests on floor top (70), got {}", b.pos.y);
        assert!(b.vel.y.abs() < 30.0);
        assert!(b.on_floor);
    }

    #[test]
    fn character_slides_along_wall() {
        let mut w = World::default();
        w.bodies.push(Body::new(1, BodyKind::Static, Shape::Rect { half: vec2(10.0, 200.0) }, vec2(50.0, 0.0)));
        let mut c = Body::new(2, BodyKind::Character, Shape::Rect { half: vec2(10.0, 10.0) }, vec2(0.0, 0.0));
        c.vel = vec2(300.0, 100.0);
        w.bodies.push(c);
        for _ in 0..30 {
            w.step(1.0 / 60.0);
        }
        let c = &w.bodies[1];
        assert!(c.pos.x <= 30.01, "blocked by wall: {}", c.pos.x);
        assert!(c.pos.y > 40.0, "keeps sliding vertically: {}", c.pos.y);
    }

    #[test]
    fn areas_report_enter_and_exit_without_pushing() {
        let mut w = World::default();
        w.bodies.push(Body::new(1, BodyKind::Area, Shape::Circle { radius: 30.0 }, vec2(0.0, 0.0)));
        let mut c = Body::new(2, BodyKind::Character, Shape::Circle { radius: 5.0 }, vec2(-100.0, 0.0));
        c.vel = vec2(600.0, 0.0);
        w.bodies.push(c);
        let (mut enters, mut exits) = (0, 0);
        for _ in 0..40 {
            let ev = w.step(1.0 / 60.0);
            enters += ev.entered.len();
            exits += ev.exited.len();
        }
        assert_eq!((enters, exits), (1, 1));
        assert!(w.bodies[1].pos.x > 200.0, "area must not block");
    }

    #[test]
    fn masks_filter_collisions() {
        let mut w = World::default();
        let mut wall = floor();
        wall.layer = 2;
        wall.mask = 2;
        w.bodies.push(wall);
        let mut ball = Body::new(2, BodyKind::Rigid, Shape::Circle { radius: 10.0 }, vec2(0.0, 0.0));
        ball.gravity_scale = 1.0;
        w.bodies.push(ball);
        for _ in 0..120 {
            w.step(1.0 / 60.0);
        }
        assert!(w.bodies[1].pos.y > 200.0, "falls through a wall on another layer");
    }

    #[test]
    fn penetration_shapes() {
        let r = Shape::Rect { half: vec2(10.0, 10.0) };
        let c = Shape::Circle { radius: 5.0 };
        let (n, d) = penetration(&c, vec2(0.0, -12.0), &r, Vec2::ZERO).unwrap();
        assert_eq!(n, vec2(0.0, -1.0));
        assert!((d - 3.0).abs() < 1e-4);
        assert!(penetration(&c, vec2(20.0, 20.0), &r, Vec2::ZERO).is_none());
        let (n, _) = penetration(&r, vec2(15.0, 0.0), &r, Vec2::ZERO).unwrap();
        assert_eq!(n, vec2(1.0, 0.0));
    }
}
