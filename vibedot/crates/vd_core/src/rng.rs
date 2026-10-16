//! Small, fast, deterministic PRNG (PCG32). Deterministic seeds make replays
//! and tests reproducible - something `rand::thread_rng` would not give us.

use crate::math::Vec2;

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(0x853c_49e6_748f_ea9b)
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng { state: 0, inc: (seed << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        // 24 random mantissa bits -> exact float in [0,1).
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    #[inline]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    pub fn range_i(&mut self, lo: i64, hi_inclusive: i64) -> i64 {
        if hi_inclusive <= lo {
            return lo;
        }
        let span = (hi_inclusive - lo + 1) as u64;
        lo + (u64::from(self.next_u32()) * span >> 32) as i64
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Uniformly distributed point inside the unit circle scaled by `radius`.
    pub fn in_circle(&mut self, radius: f32) -> Vec2 {
        let a = self.next_f32() * std::f32::consts::TAU;
        let r = self.next_f32().sqrt() * radius;
        Vec2::new(a.cos(), a.sin()) * r
    }

    pub fn unit_vector(&mut self) -> Vec2 {
        let a = self.next_f32() * std::f32::consts::TAU;
        Vec2::new(a.cos(), a.sin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            let x = a.next_f32();
            assert_eq!(x, b.next_f32());
            assert!((0.0..1.0).contains(&x));
            let i = a.range_i(-3, 3);
            b.range_i(-3, 3);
            assert!((-3..=3).contains(&i));
        }
    }
}
