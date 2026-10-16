//! Port of Godot's `core/math/math_funcs.h` / `math_defs.h` (`Math::` namespace).
//!
//! Function names, argument order and edge-case behaviour follow Godot so code
//! and tests can be translated one-to-one. `real_t` is `f32` (Godot's default
//! single-precision build).

#![allow(clippy::float_cmp)] // exact comparisons are part of Godot's semantics

pub type Real = f32;

pub const CMP_EPSILON: f64 = 0.00001;
pub const CMP_EPSILON2: f64 = CMP_EPSILON * CMP_EPSILON;
pub const CMP_NORMALIZE_TOLERANCE: f64 = 0.000001;
/// Tolerance for unit-length checks (non-`PRECISE_MATH_CHECKS` build).
pub const UNIT_EPSILON: f64 = 0.001;

pub const SQRT2: f64 = std::f64::consts::SQRT_2;
pub const SQRT12: f64 = std::f64::consts::FRAC_1_SQRT_2;
pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;
pub const E: f64 = std::f64::consts::E;
pub const INF: f64 = f64::INFINITY;
pub const NAN: f64 = f64::NAN;

/// Godot's `SIGN` macro: -1, 0 or 1.
#[inline]
pub fn sign(v: Real) -> Real {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

#[inline]
pub fn is_finite(v: Real) -> bool {
    v.is_finite()
}

/// `Math::is_equal_approx(a, b)` - relative tolerance with an absolute floor.
#[inline]
pub fn is_equal_approx(a: Real, b: Real) -> bool {
    // Exact equality first: handles infinities.
    if a == b {
        return true;
    }
    let mut tolerance = CMP_EPSILON as Real * a.abs();
    if tolerance < CMP_EPSILON as Real {
        tolerance = CMP_EPSILON as Real;
    }
    (a - b).abs() < tolerance
}

/// `Math::is_equal_approx(a, b, tolerance)`.
#[inline]
pub fn is_equal_approx_tol(a: Real, b: Real, tolerance: Real) -> bool {
    if a == b {
        return true;
    }
    (a - b).abs() < tolerance
}

#[inline]
pub fn is_zero_approx(v: Real) -> bool {
    v.abs() < CMP_EPSILON as Real
}

/// Equal, or both NaN.
#[inline]
pub fn is_same(a: Real, b: Real) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// Positive modulo with the sign of `y` (Godot `fposmod`).
#[inline]
pub fn fposmod(x: Real, y: Real) -> Real {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value + 0.0 // turns -0.0 into 0.0
}

#[inline]
pub fn posmod(x: i64, y: i64) -> i64 {
    let mut value = x % y;
    if (value < 0 && y > 0) || (value > 0 && y < 0) {
        value += y;
    }
    value
}

#[inline]
pub fn snapped(value: Real, step: Real) -> Real {
    if step != 0.0 { (f64::from(value) / f64::from(step) + 0.5).floor() as Real * step } else { value }
}

#[inline]
pub fn lerp(from: Real, to: Real, weight: Real) -> Real {
    from + (to - from) * weight
}

#[inline]
pub fn inverse_lerp(from: Real, to: Real, value: Real) -> Real {
    (value - from) / (to - from)
}

#[inline]
pub fn remap(value: Real, istart: Real, istop: Real, ostart: Real, ostop: Real) -> Real {
    lerp(ostart, ostop, inverse_lerp(istart, istop, value))
}

pub fn cubic_interpolate(from: Real, to: Real, pre: Real, post: Real, weight: Real) -> Real {
    0.5 * ((from * 2.0)
        + (-pre + to) * weight
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * (weight * weight)
        + (-pre + 3.0 * from - 3.0 * to + post) * (weight * weight * weight))
}

/// Barry-Goldman method.
pub fn cubic_interpolate_in_time(from: Real, to: Real, pre: Real, post: Real, weight: Real, to_t: Real, pre_t: Real, post_t: Real) -> Real {
    let t = lerp(0.0, to_t, weight);
    let a1 = lerp(pre, from, if pre_t == 0.0 { 0.0 } else { (t - pre_t) / -pre_t });
    let a2 = lerp(from, to, if to_t == 0.0 { 0.5 } else { t / to_t });
    let a3 = lerp(to, post, if post_t - to_t == 0.0 { 1.0 } else { (t - to_t) / (post_t - to_t) });
    let b1 = lerp(a1, a2, if to_t - pre_t == 0.0 { 0.0 } else { (t - pre_t) / (to_t - pre_t) });
    let b2 = lerp(a2, a3, if post_t == 0.0 { 1.0 } else { t / post_t });
    lerp(b1, b2, if to_t == 0.0 { 0.5 } else { t / to_t })
}

pub fn bezier_interpolate(start: Real, control_1: Real, control_2: Real, end: Real, t: Real) -> Real {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let omt3 = omt2 * omt;
    let t2 = t * t;
    let t3 = t2 * t;
    start * omt3 + control_1 * omt2 * t * 3.0 + control_2 * omt * t2 * 3.0 + end * t3
}

pub fn bezier_derivative(start: Real, control_1: Real, control_2: Real, end: Real, t: Real) -> Real {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let t2 = t * t;
    (control_1 - start) * 3.0 * omt2 + (control_2 - control_1) * 6.0 * omt * t + (end - control_2) * 3.0 * t2
}

#[inline]
pub fn angle_difference(from: Real, to: Real) -> Real {
    let tau = TAU as Real;
    let difference = (to - from) % tau;
    (2.0 * difference) % tau - difference
}

#[inline]
pub fn lerp_angle(from: Real, to: Real, weight: Real) -> Real {
    from + angle_difference(from, to) * weight
}

pub fn smoothstep(from: Real, to: Real, s: Real) -> Real {
    if is_equal_approx(from, to) {
        return if from <= to {
            if s <= from { 0.0 } else { 1.0 }
        } else if s <= to {
            1.0
        } else {
            0.0
        };
    }
    let s = ((s - from) / (to - from)).clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

#[inline]
pub fn move_toward(from: Real, to: Real, delta: Real) -> Real {
    if (to - from).abs() <= delta { to } else { from + sign(to - from) * delta }
}

pub fn wrapf(value: Real, min: Real, max: Real) -> Real {
    let range = max - min;
    if is_zero_approx(range) {
        return min;
    }
    let result = value - (range * ((value - min) / range).floor());
    if is_equal_approx(result, max) {
        return min;
    }
    result
}

pub fn wrapi(value: i64, min: i64, max: i64) -> i64 {
    let range = max - min;
    if range == 0 { min } else { min + (((value - min) % range) + range) % range }
}

#[inline]
pub fn fract(v: Real) -> Real {
    v - v.floor()
}

pub fn pingpong(value: Real, length: Real) -> Real {
    if length != 0.0 { (fract((value - length) / (length * 2.0)) * length * 2.0 - length).abs() } else { 0.0 }
}

/// Godot's `ease()` curve (`c > 1` ease-in, `0 < c < 1` ease-out, `c < 0` in-out).
pub fn ease(x: f64, c: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    if c > 0.0 {
        if c < 1.0 { 1.0 - (1.0 - x).powf(1.0 / c) } else { x.powf(c) }
    } else if c < 0.0 {
        if x < 0.5 { (x * 2.0).powf(-c) * 0.5 } else { (1.0 - (1.0 - (x - 0.5) * 2.0).powf(-c)) * 0.5 + 0.5 }
    } else {
        0.0
    }
}

#[inline]
pub fn deg_to_rad(d: Real) -> Real {
    d * (PI as Real / 180.0)
}

#[inline]
pub fn rad_to_deg(r: Real) -> Real {
    r * (180.0 / PI as Real)
}

pub fn linear_to_db(linear: Real) -> Real {
    linear.ln() * 8.685_889
}

pub fn db_to_linear(db: Real) -> Real {
    (db * 0.115_129_255).exp()
}

/// Formats a number like Godot's `String::num(value, decimals)`:
/// `printf("%.Nf")`, then trailing zeros are removed except one after the point.
pub fn string_num(value: f64, decimals: i32) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let decimals = if decimals < 0 {
        let mut d = 14;
        if value.abs() > 10.0 {
            d -= value.abs().log10().floor() as i32;
        }
        d
    } else {
        decimals
    }
    .clamp(0, 16);
    let mut s = format!("{value:.prec$}", prec = decimals as usize);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.push('0');
        }
    }
    s
}

/// Godot's `String::num_real(float, trailing)` as used by `Vector2` printing.
pub fn string_num_real(value: Real, trailing: bool) -> String {
    if value.is_nan() || value.is_infinite() {
        return string_num(f64::from(value), 0);
    }
    if value == (value as i64) as Real {
        let i = value as i64;
        return if trailing { format!("{i}.0") } else { i.to_string() };
    }
    let mut decimals = 6;
    let abs = value.abs();
    if abs > 10.0 {
        decimals -= abs.log10().floor() as i32;
    }
    string_num(f64::from(value), decimals)
}
