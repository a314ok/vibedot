//! Tests ported from Godot's `tests/core/math/test_{vector2,rect2,transform_2d,color,math_funcs}.cpp`.
//! Test names and expected values mirror the originals one-to-one.
#![allow(clippy::float_cmp, clippy::excessive_precision, clippy::approx_constant)]

use super::math_funcs::{self as math, Real, SQRT2, SQRT12, TAU};
use super::*;

/// doctest's `Approx` (epsilon = FLT_EPSILON * 100, scaled by magnitude).
fn approx(a: Real, b: Real) -> bool {
    let eps = f32::EPSILON * 100.0;
    (a - b).abs() < eps * (1.0 + a.abs().max(b.abs()))
}

fn v(x: f64, y: f64) -> Vector2 {
    Vector2::new(x as Real, y as Real)
}

// ------------------------------------------------------------------ Vector2

#[test]
fn vector2_constructor_methods() {
    assert_eq!(Vector2::default(), v(0.0, 0.0));
}

#[test]
fn vector2_angle_methods() {
    let vx = v(1.0, 0.0);
    let vy = v(0.0, 1.0);
    assert!(approx(vx.angle_to(vy), (TAU / 4.0) as Real));
    assert!(approx(vy.angle_to(vx), (-TAU / 4.0) as Real));
    assert!(approx(vx.angle_to_point(vy), (TAU * 3.0 / 8.0) as Real));
    assert!(approx(vy.angle_to_point(vx), (-TAU / 8.0) as Real));
}

#[test]
fn vector2_axis_methods() {
    let mut vector = v(1.2, 3.4);
    assert_eq!(vector.max_axis_index(), Axis::Y);
    assert_eq!(vector.min_axis_index(), Axis::X);
    assert_eq!(vector[vector.min_axis_index()], 1.2);
    vector[Axis::Y] = 3.7;
    assert_eq!(vector[Axis::Y], 3.7);
}

#[test]
fn vector2_interpolation_methods() {
    let vector1 = v(1.0, 2.0);
    let vector2 = v(4.0, 5.0);
    assert_eq!(vector1.lerp(vector2, 0.5), v(2.5, 3.5));
    assert!(vector1.lerp(vector2, 1.0 / 3.0).is_equal_approx(v(2.0, 3.0)));
    assert!(vector1.normalized().slerp(vector2.normalized(), 0.5).is_equal_approx(v(0.538953602313995361, 0.84233558177947998)));
    assert!(vector1.normalized().slerp(vector2.normalized(), 1.0 / 3.0).is_equal_approx(v(0.508990883827209473, 0.860771894454956055)));
    assert!(v(5.0, 0.0).slerp(v(0.0, 5.0), 0.5).is_equal_approx(v(5.0, 5.0) * SQRT12 as Real));
    assert!(v(1.0, 1.0).slerp(v(2.0, 2.0), 0.5).is_equal_approx(v(1.5, 1.5)));
    assert_eq!(Vector2::ZERO.slerp(Vector2::ZERO, 0.5), Vector2::ZERO);
    assert_eq!(Vector2::ZERO.slerp(v(1.0, 1.0), 0.5), v(0.5, 0.5));
    assert_eq!(v(1.0, 1.0).slerp(Vector2::ZERO, 0.5), v(0.5, 0.5));
    assert!(v(4.0, 6.0).slerp(v(8.0, 10.0), 0.5).is_equal_approx(v(5.9076470794008017626, 8.07918879020090480697)));
    assert!(approx(vector1.slerp(vector2, 0.5).length(), 4.31959610746631919));
    assert!(approx(vector1.angle_to(vector1.slerp(vector2, 0.5)) * 2.0, vector1.angle_to(vector2)));
    assert_eq!(vector1.cubic_interpolate(vector2, Vector2::ZERO, v(7.0, 7.0), 0.5), v(2.375, 3.5));
    assert!(vector1
        .cubic_interpolate(vector2, Vector2::ZERO, v(7.0, 7.0), 1.0 / 3.0)
        .is_equal_approx(v(1.851851940155029297, 2.962963104248046875)));
    assert_eq!(v(1.0, 0.0).move_toward(v(10.0, 0.0), 3.0), v(4.0, 0.0));
}

#[test]
fn vector2_length_methods() {
    let vector1 = v(10.0, 10.0);
    let vector2 = v(20.0, 30.0);
    assert_eq!(vector1.length_squared(), 200.0);
    assert!(approx(vector1.length(), 10.0 * SQRT2 as Real));
    assert_eq!(vector2.length_squared(), 1300.0);
    assert!(approx(vector2.length(), 36.05551275463989293119));
    assert_eq!(vector1.distance_squared_to(vector2), 500.0);
    assert!(approx(vector1.distance_to(vector2), 22.36067977499789696409));
}

#[test]
fn vector2_limiting_methods() {
    let vector = v(10.0, 10.0);
    assert!(vector.limit_length().is_equal_approx(v(SQRT12, SQRT12)));
    assert!(vector.limit_length_to(5.0).is_equal_approx(v(SQRT12, SQRT12) * 5.0));
    assert!(v(-5.0, 15.0).clamp(Vector2::ZERO, vector).is_equal_approx(v(0.0, 10.0)));
    assert!(vector.clamp(v(0.0, 15.0), v(5.0, 20.0)).is_equal_approx(v(5.0, 15.0)));
}

#[test]
fn vector2_normalization_methods() {
    assert!(v(1.0, 0.0).is_normalized());
    assert!(!v(1.0, 1.0).is_normalized());
    assert_eq!(v(1.0, 0.0).normalized(), v(1.0, 0.0));
    assert!(v(1.0, 1.0).normalized().is_equal_approx(v(SQRT12, SQRT12)));
    let mut vector = v(3.2, -5.4);
    vector.normalize();
    assert_eq!(vector, v(3.2, -5.4).normalized());
    assert!(vector.is_equal_approx(v(0.509802390301732898898, -0.860291533634174266891)));
}

#[test]
fn vector2_operators() {
    let decimal1 = v(2.3, 4.9);
    let decimal2 = v(1.2, 3.4);
    let power1 = v(0.75, 1.5);
    let power2 = v(0.5, 0.125);
    let int1 = v(4.0, 5.0);
    let int2 = v(1.0, 2.0);
    assert!((decimal1 + decimal2).is_equal_approx(v(3.5, 8.3)));
    assert_eq!(power1 + power2, v(1.25, 1.625));
    assert_eq!(int1 + int2, v(5.0, 7.0));
    assert!((decimal1 - decimal2).is_equal_approx(v(1.1, 1.5)));
    assert_eq!(power1 - power2, v(0.25, 1.375));
    assert_eq!(int1 - int2, v(3.0, 3.0));
    assert!((decimal1 * decimal2).is_equal_approx(v(2.76, 16.66)));
    assert_eq!(power1 * power2, v(0.375, 0.1875));
    assert_eq!(int1 * int2, v(4.0, 10.0));
    assert!((decimal1 / decimal2).is_equal_approx(v(1.91666666666666666, 1.44117647058823529)));
    assert_eq!(power1 / power2, v(1.5, 12.0));
    assert_eq!(int1 / int2, v(4.0, 2.5));
    assert!((decimal1 * 2.0).is_equal_approx(v(4.6, 9.8)));
    assert_eq!(power1 * 2.0, v(1.5, 3.0));
    assert_eq!(int1 * 2.0, v(8.0, 10.0));
    assert!((decimal1 / 2.0).is_equal_approx(v(1.15, 2.45)));
    assert_eq!(power1 / 2.0, v(0.375, 0.75));
    assert_eq!(int1 / 2.0, v(2.0, 2.5));
    assert_eq!(decimal1.to_vector2i(), (2, 4));
    assert_eq!(decimal2.to_vector2i(), (1, 3));
    assert_eq!(decimal1.to_string(), "(2.3, 4.9)");
    assert_eq!(decimal2.to_string(), "(1.2, 3.4)");
    assert_eq!(v(9.8, 9.9).to_string(), "(9.8, 9.9)");
    assert_eq!(v(math::PI, TAU).to_string(), "(3.141593, 6.283185)");
}

#[test]
fn vector2_other_methods() {
    let vector = v(1.2, 3.4);
    assert!(approx(vector.aspect(), 1.2 / 3.4));
    assert!(vector.direction_to(Vector2::ZERO).is_equal_approx(-vector.normalized()));
    assert!(v(1.0, 1.0).direction_to(v(2.0, 2.0)).is_equal_approx(v(SQRT12, SQRT12)));
    assert!(vector.inverse().is_equal_approx(v(1.0 / 1.2, 1.0 / 3.4)));
    assert!(vector.posmod(2.0).is_equal_approx(v(1.2, 1.4)));
    assert!((-vector).posmod(2.0).is_equal_approx(v(0.8, 0.6)));
    assert!(vector.posmodv(v(1.0, 2.0)).is_equal_approx(v(0.2, 1.4)));
    assert!((-vector).posmodv(v(2.0, 3.0)).is_equal_approx(v(0.8, 2.6)));
    assert!(vector.rotated(TAU as Real).is_equal_approx(v(1.2, 3.4)));
    assert!(vector.rotated((TAU / 4.0) as Real).is_equal_approx(v(-3.4, 1.2)));
    assert!(vector.rotated((TAU / 3.0) as Real).is_equal_approx(v(-3.544486372867091398996, -0.660769515458673623883)));
    assert!(vector.rotated((TAU / 2.0) as Real).is_equal_approx(vector.rotated((TAU / -2.0) as Real)));
    assert_eq!(vector.snapped(v(1.0, 1.0)), v(1.0, 3.0));
    assert_eq!(v(3.4, 5.6).snapped(v(1.0, 1.0)), v(3.0, 6.0));
    assert_eq!(vector.snapped(v(0.25, 0.25)), v(1.25, 3.5));
    assert!(v(1.2, 2.5).is_equal_approx(vector.min(v(3.0, 2.5))));
    assert!(v(5.3, 3.4).is_equal_approx(vector.max(v(5.3, 2.0))));
}

#[test]
fn vector2_plane_methods() {
    let vector = v(1.2, 3.4);
    let vector_y = v(0.0, 1.0);
    let vector_normal = v(0.95879811270838721622267, 0.2840883296913739899919);
    let p_d = 99.1;
    assert_eq!(vector.bounce(vector_y), v(1.2, -3.4));
    assert!(vector.bounce(vector_normal).is_equal_approx(v(-2.85851197982345523329, 2.197477931904161412358)));
    assert_eq!(vector.reflect(vector_y), v(-1.2, 3.4));
    assert!(vector.reflect(vector_normal).is_equal_approx(v(2.85851197982345523329, -2.197477931904161412358)));
    assert_eq!(vector.project(vector_y), v(0.0, 3.4));
    assert!(vector.project(vector_normal).is_equal_approx(v(2.0292559899117276166, 0.60126103404791929382)));
    assert!(vector_normal.plane_project(p_d, vector).is_equal_approx(v(94.187635516479631, 30.951892004882851)));
    assert_eq!(vector.slide(vector_y), v(1.2, 0.0));
    assert!(vector.slide(vector_normal).is_equal_approx(v(-0.8292559899117276166456, 2.798738965952080706179)));
    // MATH_CHECKS: non-normalized normals yield (0, 0) in debug builds.
    if cfg!(debug_assertions) {
        let non_normal = v(5.4, 1.6);
        assert!(vector.bounce(non_normal).is_equal_approx(Vector2::ZERO));
        assert!(vector.reflect(non_normal).is_equal_approx(Vector2::ZERO));
        assert!(vector.slide(non_normal).is_equal_approx(Vector2::ZERO));
    }
}

#[test]
fn vector2_rounding_methods() {
    let vector1 = v(1.2, 5.6);
    let vector2 = v(1.2, -5.6);
    assert_eq!(vector1.abs(), vector1);
    assert_eq!(vector2.abs(), vector1);
    assert_eq!(vector1.ceil(), v(2.0, 6.0));
    assert_eq!(vector2.ceil(), v(2.0, -5.0));
    assert_eq!(vector1.floor(), v(1.0, 5.0));
    assert_eq!(vector2.floor(), v(1.0, -6.0));
    assert_eq!(vector1.round(), v(1.0, 6.0));
    assert_eq!(vector2.round(), v(1.0, -6.0));
    assert_eq!(vector1.sign(), v(1.0, 1.0));
    assert_eq!(vector2.sign(), v(1.0, -1.0));
}

#[test]
fn vector2_linear_algebra_methods() {
    let vx = v(1.0, 0.0);
    let vy = v(0.0, 1.0);
    let a = v(3.5, 8.5);
    let b = v(5.2, 4.6);
    assert_eq!(vx.cross(vy), 1.0);
    assert_eq!(vy.cross(vx), -1.0);
    assert!(approx(a.cross(b), -28.1));
    assert!(approx(v(-3.5, 8.5).cross(v(5.2, -4.6)), -28.1));
    assert_eq!(vx.dot(vy), 0.0);
    assert_eq!(vx.dot(vx), 1.0);
    assert_eq!((vx * 10.0).dot(vx * 10.0), 100.0);
    assert!(approx(a.dot(b), 57.3));
    assert!(approx(v(-3.5, 8.5).dot(v(5.2, -4.6)), -57.3));
}

#[test]
fn vector2_finite_number_checks() {
    let infinite = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    assert!(v(0.0, 1.0).is_finite());
    for x in infinite {
        assert!(!v(x, 1.0).is_finite());
        assert!(!v(0.0, x).is_finite());
        for y in infinite {
            assert!(!v(x, y).is_finite());
        }
    }
}

// ------------------------------------------------------------------ Rect2

#[test]
fn rect2_string_conversion() {
    assert_eq!(Rect2::new(0.0, 100.0, 1280.0, 720.0).to_string(), "[P: (0.0, 100.0), S: (1280.0, 720.0)]");
}

#[test]
fn rect2_getters_setters() {
    let rect = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert_eq!(rect, Rect2::from_pos_size(v(0.0, 100.0), v(1280.0, 720.0)));
    assert!(rect.get_end().is_equal_approx(v(1280.0, 820.0)));
    assert!(rect.get_center().is_equal_approx(v(640.0, 460.0)));
    assert!(Rect2::new(0.0, 100.0, 1281.0, 721.0).get_center().is_equal_approx(v(640.5, 460.5)));
    let mut r = rect;
    r.set_end(v(4000.0, 4000.0));
    assert!(r.is_equal_approx(&Rect2::new(0.0, 100.0, 4000.0, 3900.0)));
}

#[test]
fn rect2_area_getters() {
    assert!(approx(Rect2::new(0.0, 100.0, 1280.0, 720.0).get_area(), 921_600.0));
    assert!(approx(Rect2::new(0.0, 100.0, -1280.0, -720.0).get_area(), 921_600.0));
    assert!(approx(Rect2::new(0.0, 100.0, 1280.0, -720.0).get_area(), -921_600.0));
    assert!(approx(Rect2::new(0.0, 100.0, -1280.0, 720.0).get_area(), -921_600.0));
    assert!(math::is_zero_approx(Rect2::new(0.0, 100.0, 0.0, 720.0).get_area()));
    assert!(Rect2::new(0.0, 100.0, 1280.0, 720.0).has_area());
    assert!(!Rect2::new(0.0, 100.0, 0.0, 500.0).has_area());
    assert!(!Rect2::new(0.0, 100.0, 500.0, 0.0).has_area());
    assert!(!Rect2::new(0.0, 100.0, 0.0, 0.0).has_area());
}

#[test]
fn rect2_absolute_coordinates() {
    assert!(Rect2::new(0.0, 100.0, 1280.0, 720.0).abs().is_equal_approx(&Rect2::new(0.0, 100.0, 1280.0, 720.0)));
    assert!(Rect2::new(0.0, -100.0, 1280.0, 720.0).abs().is_equal_approx(&Rect2::new(0.0, -100.0, 1280.0, 720.0)));
    assert!(Rect2::new(0.0, -100.0, -1280.0, -720.0).abs().is_equal_approx(&Rect2::new(-1280.0, -820.0, 1280.0, 720.0)));
    assert!(Rect2::new(0.0, 100.0, -1280.0, 720.0).abs().is_equal_approx(&Rect2::new(-1280.0, 100.0, 1280.0, 720.0)));
}

#[test]
fn rect2_intersection() {
    let r = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert!(r.intersection(&Rect2::new(0.0, 300.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(0.0, 300.0, 100.0, 100.0)));
    assert!(r.intersection(&Rect2::new(1200.0, 700.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(1200.0, 700.0, 80.0, 100.0)));
    assert!(r.intersection(&Rect2::new(-4000.0, -4000.0, 100.0, 100.0)).is_equal_approx(&Rect2::default()));
    assert!(r.intersects(&Rect2::new(0.0, 300.0, 100.0, 100.0), false));
    assert!(r.intersects(&Rect2::new(1200.0, 700.0, 100.0, 100.0), false));
    assert!(!r.intersects(&Rect2::new(-4000.0, -4000.0, 100.0, 100.0), false));
}

#[test]
fn rect2_intersection_with_transformed_rect() {
    let rect = Rect2::new(0.0, 100.0, 400.0, 300.0);
    let s2 = SQRT2 as Real;
    // Translated.
    let xform = Transform2D::from_rotation(0.0, v(100.0, 100.0));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(0.0, 100.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(100.0, 200.0, 100.0, 100.0)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(250.0, 150.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(350.0, 250.0, 50.0, 100.0)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(350.0, 350.0, 100.0, 100.0)).is_equal_approx(&Rect2::default()));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(-150.0, -50.0, 500.0, 400.0)).is_equal_approx(&rect));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(0.0, -150.0, 100.0, 500.0)).is_equal_approx(&Rect2::new(100.0, 100.0, 100.0, 300.0)));
    // Scaled.
    let xform = Transform2D::from_rotation_scale_skew(0.0, v(2.0, 2.0), 0.0, Vector2::ZERO);
    assert!(rect.intersection_transformed(&xform, &Rect2::new(50.0, 75.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(100.0, 150.0, 200.0, 200.0)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(175.0, 150.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(350.0, 300.0, 50.0, 100.0)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(250.0, 220.0, 100.0, 100.0)).is_equal_approx(&Rect2::default()));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(-100.0, -50.0, 500.0, 400.0)).is_equal_approx(&rect));
    // Rotated.
    let xform = Transform2D::from_rotation((math::PI / 4.0) as Real, Vector2::ZERO);
    let r = rect.intersection_transformed(&xform, &Rect2::new(100.0, 50.0, 20.0, 20.0));
    assert!(r.is_equal_approx(&Rect2::new(15.0 * s2, 75.0 * s2, 20.0 * s2, 20.0 * s2)), "{r}");
    assert!(rect
        .intersection_transformed(&xform, &Rect2::new(80.0, 90.0, 20.0, 20.0))
        .is_equal_approx(&Rect2::new(0.0, 90.0 * s2, 5.0 * s2, 10.0 * s2)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(575.0, -50.0, 100.0, 100.0)).is_equal_approx(&Rect2::default()));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(0.0, -250.0, 600.0, 600.0)).is_equal_approx(&rect));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(0.0, -250.0, 1000.0, 1000.0)).is_equal_approx(&rect));
    assert!(rect
        .intersection_transformed(&xform, &Rect2::new(75.0, 25.0, 500.0, 10.0))
        .is_equal_approx(&Rect2::new(100.0 - 35.0 * s2, 100.0, 300.0 + 10.0 * s2, 300.0)));
    let expected = Rect2::new(100.0 + 90.0 * s2, 100.0, 300.0 - 90.0 * s2, 300.0 - 90.0 * s2);
    for r in [
        Rect2::new(-50.0, -100.0, 600.0, 10.0),
        Rect2::new(-50.0, -100.0, 1000.0, 10.0),
        Rect2::new(-95.0, -100.0, 600.0, 10.0),
        Rect2::new(-95.0, -100.0, 1000.0, 10.0),
        Rect2::new(-200.0, -100.0, 1000.0, 10.0),
    ] {
        assert!(rect.intersection_transformed(&xform, &r).is_equal_approx(&expected), "{r}");
    }
    // Skewed.
    let xform = Transform2D::from_rotation_scale_skew(0.0, v(1.0, 1.0), (math::PI / 4.0) as Real, Vector2::ZERO);
    assert!(rect
        .intersection_transformed(&xform, &Rect2::new(300.0, 200.0, 100.0, 50.0))
        .is_equal_approx(&Rect2::new(300.0 - 125.0 * s2, 100.0 * s2, 100.0 + 25.0 * s2, 25.0 * s2)));
    assert!(rect
        .intersection_transformed(&xform, &Rect2::new(550.0, 200.0, 100.0, 50.0))
        .is_equal_approx(&Rect2::new(550.0 - 125.0 * s2, 150.0, -150.0 + 125.0 * s2, -150.0 + 125.0 * s2)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(810.0, 550.0, 100.0, 50.0)).is_equal_approx(&Rect2::default()));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(0.0, 0.0, 2000.0, 1000.0)).is_equal_approx(&rect));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(-3000.0, 0.0, 4000.0, 3000.0)).is_equal_approx(&rect));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(-150.0, 130.0, 600.0, 450.0)).is_equal_approx(&Rect2::new(0.0, 100.0, 350.0, 300.0)));
    assert!(rect.intersection_transformed(&xform, &Rect2::new(450.0, 130.0, 600.0, 450.0)).is_equal_approx(&Rect2::new(50.0, 100.0, 350.0, 300.0)));
}

#[test]
fn rect2_enclosing_expanding_support() {
    let r = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert!(r.encloses(&Rect2::new(0.0, 300.0, 100.0, 100.0)));
    assert!(!r.encloses(&Rect2::new(1200.0, 700.0, 100.0, 100.0)));
    assert!(!r.encloses(&Rect2::new(-4000.0, -4000.0, 100.0, 100.0)));
    assert!(r.expand(v(500.0, 600.0)).is_equal_approx(&r));
    assert!(r.expand(v(0.0, 0.0)).is_equal_approx(&Rect2::new(0.0, 0.0, 1280.0, 820.0)));
    let rect = Rect2::from_pos_size(v(-1.5, 2.0), v(4.0, 5.0));
    assert_eq!(rect.get_support(v(1.0, 0.0)), v(2.5, 2.0));
    assert_eq!(rect.get_support(v(0.5, 1.0)), v(2.5, 7.0));
    assert_eq!(rect.get_support(v(0.0, -1.0)), v(-1.5, 2.0));
    assert_eq!(rect.get_support(v(0.0, -0.1)), v(-1.5, 2.0));
    assert_eq!(rect.get_support(Vector2::ZERO), v(-1.5, 2.0));
}

#[test]
fn rect2_growing() {
    let r = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert!(r.grow(100.0).is_equal_approx(&Rect2::new(-100.0, 0.0, 1480.0, 920.0)));
    assert!(r.grow(-100.0).is_equal_approx(&Rect2::new(100.0, 200.0, 1080.0, 520.0)));
    assert!(r.grow(-4000.0).is_equal_approx(&Rect2::new(4000.0, 4100.0, -6720.0, -7280.0)));
    assert!(r.grow_individual(100.0, 200.0, 300.0, 400.0).is_equal_approx(&Rect2::new(-100.0, -100.0, 1680.0, 1320.0)));
    assert!(r.grow_individual(-100.0, 200.0, 300.0, -400.0).is_equal_approx(&Rect2::new(100.0, -100.0, 1480.0, 520.0)));
    assert!(r.grow_side(Side::Top, 500.0).is_equal_approx(&Rect2::new(0.0, -400.0, 1280.0, 1220.0)));
    assert!(r.grow_side(Side::Top, -500.0).is_equal_approx(&Rect2::new(0.0, 600.0, 1280.0, 220.0)));
}

#[test]
fn rect2_has_point() {
    let rect = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert!(rect.has_point(v(500.0, 600.0)));
    assert!(!rect.has_point(v(0.0, 0.0)));
    assert!(rect.has_point(rect.position));
    assert!(rect.has_point(rect.position + v(1.0, 1.0)));
    assert!(!rect.has_point(rect.position + v(1.0, -1.0)));
    assert!(!rect.has_point(rect.position + rect.size));
    assert!(!rect.has_point(rect.position + rect.size + v(1.0, 1.0)));
    assert!(rect.has_point(rect.position + rect.size + v(-1.0, -1.0)));
    assert!(!rect.has_point(rect.position + rect.size + v(-1.0, 1.0)));
    assert!(rect.has_point(rect.position + v(0.0, 10.0)));
    assert!(!rect.has_point(rect.position + v(f64::from(rect.size.x), 10.0)));
    assert!(rect.has_point(rect.position + v(10.0, 0.0)));
    assert!(!rect.has_point(rect.position + v(10.0, f64::from(rect.size.y))));
    let rect = Rect2::new(-4000.0, -200.0, 1280.0, 720.0);
    assert!(rect.has_point(rect.position + v(0.0, 10.0)));
    assert!(!rect.has_point(rect.position + v(f64::from(rect.size.x), 10.0)));
    assert!(rect.has_point(rect.position + v(10.0, 0.0)));
    assert!(!rect.has_point(rect.position + v(10.0, f64::from(rect.size.y))));
}

#[test]
fn rect2_merging_and_finite() {
    let r = Rect2::new(0.0, 100.0, 1280.0, 720.0);
    assert!(r.merge(&Rect2::new(0.0, 300.0, 100.0, 100.0)).is_equal_approx(&r));
    assert!(r.merge(&Rect2::new(1200.0, 700.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(0.0, 100.0, 1300.0, 720.0)));
    assert!(r.merge(&Rect2::new(-4000.0, -4000.0, 100.0, 100.0)).is_equal_approx(&Rect2::new(-4000.0, -4000.0, 5280.0, 4820.0)));
    let x = v(0.0, 1.0);
    let inf = v(f64::NAN, f64::NAN);
    assert!(Rect2::from_pos_size(x, x).is_finite());
    assert!(!Rect2::from_pos_size(inf, x).is_finite());
    assert!(!Rect2::from_pos_size(x, inf).is_finite());
    assert!(!Rect2::from_pos_size(inf, inf).is_finite());
}

// ------------------------------------------------------------------ Transform2D

fn dummy() -> Transform2D {
    Transform2D::from_columns(v(1.0, 2.0), v(3.0, 4.0), v(5.0, 6.0))
}

#[test]
fn transform2d_constructors() {
    assert_eq!(Transform2D::default(), Transform2D::from_columns(v(1.0, 0.0), v(0.0, 1.0), v(0.0, 0.0)));
    let rotation = (math::PI / 4.0) as Real;
    let translation = v(20.0, -20.0);
    assert_eq!(Transform2D::from_rotation(rotation, translation), Transform2D::IDENTITY.rotated(rotation).translated(translation));
    let rotation = (math::PI / 2.0) as Real;
    let scale = v(2.0, 0.5);
    let skew = (math::PI / 4.0) as Real;
    let translation = v(30.0, 0.0);
    let test = Transform2D::from_rotation_scale_skew(rotation, scale, skew, translation);
    let mut expected = Transform2D::IDENTITY.scaled(scale).rotated(rotation).translated(translation);
    expected.set_skew(skew);
    assert!(test.is_equal_approx(&expected));
    assert_eq!(Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0), dummy());
}

#[test]
fn transform2d_xform_and_inverse() {
    let t = dummy();
    assert_eq!(t.xform(v(2.0, 3.0)), v(1.0 * 2.0 + 3.0 * 3.0 + 5.0, 2.0 * 2.0 + 4.0 * 3.0 + 6.0));
    let t1 = Transform2D::from_columns(v(1.0, 2.0), v(3.0, 4.0), Vector2::ZERO);
    assert_eq!(t1.basis_xform(v(2.0, 2.0)), t1.xform(v(2.0, 2.0)));
    assert_ne!(t.basis_xform(v(2.0, 2.0)), t.xform(v(2.0, 2.0)));
    assert_eq!(t.affine_inverse().affine_inverse(), t);
}

#[test]
fn transform2d_orthonormalized() {
    let o = dummy().orthonormalized();
    assert!(math::is_equal_approx(o.columns[0].length_squared(), 1.0));
    assert!(math::is_equal_approx(o.columns[1].length_squared(), 1.0));
    let vx = v(f64::from(o.columns[0].x), f64::from(o.columns[1].x));
    let vy = v(f64::from(o.columns[0].y), f64::from(o.columns[1].y));
    assert!(math::is_equal_approx(o.basis_xform(vx).x, 1.0));
    assert!(math::is_equal_approx(o.basis_xform(vy).x, 0.0));
    assert!(math::is_equal_approx(o.basis_xform(vx).y, 0.0));
    assert!(math::is_equal_approx(o.basis_xform(vy).y, 1.0));
}

#[test]
fn transform2d_translation_scaling_rotation() {
    let id = Transform2D::IDENTITY;
    let orig = dummy();
    let offset = v(1.0, 2.0);
    assert_eq!(id.translated(offset), id.translated_local(offset));
    let t = id.translated(offset);
    assert_eq!(orig.translated(offset), t * orig);
    assert_eq!(orig.translated_local(offset), orig * t);
    let scaling = v(1.0, 2.0);
    assert_eq!(id.scaled(scaling), id.scaled_local(scaling));
    let s = id.scaled(scaling);
    assert_eq!(orig.scaled(scaling), s * orig);
    assert_eq!(orig.scaled_local(scaling), orig * s);
    let phi = 1.0;
    assert_eq!(id.rotated(phi), id.rotated_local(phi));
    let r = id.rotated(phi);
    assert_eq!(orig.rotated(phi), r * orig);
    assert_eq!(orig.rotated_local(phi), orig * r);
}

#[test]
fn transform2d_interpolation() {
    let full = Transform2D::from_rotation_scale_skew(math::deg_to_rad(170.0), v(3.6, 8.0), math::deg_to_rad(20.0), v(2.4, 6.8));
    let half = Transform2D::from_rotation_scale_skew(math::deg_to_rad(85.0), v(2.3, 4.5), math::deg_to_rad(10.0), v(1.2, 3.4));
    let i = Transform2D::IDENTITY.interpolate_with(&full, 0.5);
    assert!(i.get_origin().is_equal_approx(half.get_origin()));
    assert!(approx(i.get_rotation(), half.get_rotation()));
    assert!(i.get_scale().is_equal_approx(half.get_scale()));
    assert!(approx(i.get_skew(), half.get_skew()));
    assert!(i.is_equal_approx(&half));
    assert!(full.interpolate_with(&Transform2D::IDENTITY, 0.5).is_equal_approx(&half));
}

#[test]
fn transform2d_finite_and_conformal() {
    let x = v(0.0, 1.0);
    let inf = v(f64::NAN, f64::NAN);
    assert!(Transform2D::from_columns(x, x, x).is_finite());
    for (a, b, c) in [(inf, x, x), (x, inf, x), (x, x, inf), (inf, inf, x), (inf, x, inf), (x, inf, inf), (inf, inf, inf)] {
        assert!(!Transform2D::from_columns(a, b, c).is_finite());
    }
    assert!(Transform2D::IDENTITY.is_conformal());
    assert!(Transform2D::from_rotation(1.2, Vector2::ZERO).is_conformal());
    assert!(Transform2D::from_columns(v(1.0, 0.0), v(0.0, -1.0), Vector2::ZERO).is_conformal());
    assert!(Transform2D::from_columns(v(1.2, 0.0), v(0.0, 1.2), Vector2::ZERO).is_conformal());
    assert!(Transform2D::from_columns(v(1.2, 3.4), v(3.4, -1.2), Vector2::ZERO).is_conformal());
    assert!(!Transform2D::from_columns(v(1.2, 0.0), v(0.0, 3.4), Vector2::ZERO).is_conformal());
    assert!(!Transform2D::from_columns(v(SQRT12, SQRT12), v(0.0, 1.0), Vector2::ZERO).is_conformal());
}

#[test]
fn transform2d_affine_roundtrip() {
    let t = Transform2D::from_rotation_scale_skew(0.3, v(2.0, -1.5), 0.1, v(10.0, 20.0));
    assert!(Transform2D::from_affine(&t.to_affine()).is_equal_approx(&t));
    let p = v(3.0, 4.0);
    let a: glam::Vec2 = t.xform(p).into();
    assert!(a.abs_diff_eq(t.to_affine().transform_point2(p.into()), 1e-4));
}

// ------------------------------------------------------------------ Color

#[test]
fn color_constructor_methods() {
    let blue_rgba = GdColor::new(0.25098, 0.376471, 1.0, 0.501961);
    assert!(blue_rgba.is_equal_approx(&GdColor::html("#4060ff80")));
    assert!(blue_rgba.is_equal_approx(&GdColor::hex(0x4060ff80)));
    assert!(blue_rgba.is_equal_approx(&GdColor::hex64(0x4040_6060_ffff_8080)));
    assert!(GdColor::html("invalid").is_equal_approx(&GdColor::default()));
    let green_rgba = GdColor::new(0.0, 1.0, 0.0, 0.25);
    assert!(green_rgba.is_equal_approx(&GdColor::from_hsv(120.0 / 360.0, 1.0, 1.0, 0.25)));
}

#[test]
fn color_operators() {
    let blue = GdColor::rgb(0.2, 0.2, 1.0);
    let dark_red = GdColor::rgb(0.3, 0.1, 0.1);
    assert!((blue + dark_red).is_equal_approx(&GdColor::new(0.5, 0.3, 1.1, 2.0)));
    assert!((blue - dark_red).is_equal_approx(&GdColor::new(-0.1, 0.1, 0.9, 0.0)));
    assert!((blue * 2.0).is_equal_approx(&GdColor::new(0.4, 0.4, 2.0, 2.0)));
    assert!((blue / 2.0).is_equal_approx(&GdColor::new(0.1, 0.1, 0.5, 0.5)));
    assert!((blue * dark_red).is_equal_approx(&GdColor::rgb(0.06, 0.02, 0.1)));
    assert!((blue / dark_red).is_equal_approx(&GdColor::rgb(0.666667, 2.0, 10.0)));
    assert!((-blue).is_equal_approx(&GdColor::new(0.8, 0.8, 0.0, 0.0)));
}

#[test]
fn color_reading_and_conversion() {
    let dark_blue = GdColor::new(0.0, 0.0, 0.5, 0.4);
    assert!(approx(dark_blue.get_h(), 240.0 / 360.0));
    assert!(approx(dark_blue.get_s(), 1.0));
    assert!(approx(dark_blue.get_v(), 0.5));
    let cyan = GdColor::rgb(0.0, 1.0, 1.0);
    let cyan_t = GdColor::new(0.0, 1.0, 1.0, 0.0);
    assert_eq!(cyan.to_html(true), "00ffffff");
    assert_eq!(cyan_t.to_html(true), "00ffff00");
    assert_eq!(cyan.to_argb32(), 0xff00ffff);
    assert_eq!(cyan.to_abgr32(), 0xffffff00);
    assert_eq!(cyan.to_rgba32(), 0x00ffffff);
    assert_eq!(cyan.to_argb64(), 0xffff_0000_ffff_ffff);
    assert_eq!(cyan.to_abgr64(), 0xffff_ffff_ffff_0000);
    assert_eq!(cyan.to_rgba64(), 0x0000_ffff_ffff_ffff);
    assert_eq!(cyan.to_string(), "(0.0, 1.0, 1.0, 1.0)");
}

#[test]
fn color_linear_srgb() {
    let color = GdColor::new(0.35, 0.5, 0.6, 0.7);
    let lin = color.srgb_to_linear();
    let srgb = color.linear_to_srgb();
    assert!(lin.is_equal_approx(&GdColor::new(0.100481, 0.214041, 0.318547, 0.7)));
    assert!(srgb.is_equal_approx(&GdColor::new(0.62621, 0.735357, 0.797738, 0.7)));
    assert!(lin.linear_to_srgb().is_equal_approx(&color));
    assert!(srgb.srgb_to_linear().is_equal_approx(&color));
    assert_eq!(GdColor::new(1.0, 1.0, 1.0, 1.0).srgb_to_linear(), GdColor::new(1.0, 1.0, 1.0, 1.0));
    assert_eq!(GdColor::new(1.0, 1.0, 1.0, 1.0).linear_to_srgb(), GdColor::new(1.0, 1.0, 1.0, 1.0));
}

#[test]
fn color_named_and_validation() {
    assert!(GdColor::named("red").is_equal_approx(&GdColor::hex(0xFF0000FF)));
    assert!(GdColor::named("white_smoke").is_equal_approx(&GdColor::hex(0xF5F5F5FF)));
    assert!(GdColor::named("Slate Blue").is_equal_approx(&GdColor::hex(0x6A5ACDFF)));
    assert!(GdColor::named("doesn't exist").is_equal_approx(&GdColor::default()));
    assert!(GdColor::html_is_valid("#4080ff"));
    assert!(GdColor::html_is_valid("4080ff"));
    assert!(!GdColor::html_is_valid("12345"));
    assert!(!GdColor::html_is_valid("#fuf"));
}

#[test]
fn color_manipulation() {
    let blue = GdColor::new(0.0, 0.0, 1.0, 0.4);
    assert!(blue.inverted().is_equal_approx(&GdColor::new(1.0, 1.0, 0.0, 0.4)));
    let purple = GdColor::new(0.5, 0.2, 0.5, 0.25);
    assert!(purple.lightened(0.2).is_equal_approx(&GdColor::new(0.6, 0.36, 0.6, 0.25)));
    assert!(purple.darkened(0.2).is_equal_approx(&GdColor::new(0.4, 0.16, 0.4, 0.25)));
    let red = GdColor::new(1.0, 0.0, 0.0, 0.2);
    let yellow = GdColor::new(1.0, 1.0, 0.0, 0.8);
    assert!(red.lerp(yellow, 0.5).is_equal_approx(&GdColor::new(1.0, 0.5, 0.0, 0.5)));
}

// ------------------------------------------------------------------ Math

#[test]
fn math_funcs_subset() {
    assert_eq!(math::fposmod(-5.5, 3.0), 0.5);
    assert_eq!(math::fposmod(5.5, -3.0), -0.5);
    assert_eq!(math::fposmod(-0.0, 3.0).to_bits(), 0.0f32.to_bits());
    assert_eq!(math::posmod(-7, 3), 2);
    assert_eq!(math::snapped(3.4, 1.0), 3.0);
    assert_eq!(math::snapped(3.5, 1.0), 4.0);
    assert_eq!(math::snapped(3.7, 0.0), 3.7);
    assert!(approx(math::wrapf(370.0, 0.0, 360.0), 10.0));
    assert_eq!(math::wrapf(360.0, 0.0, 360.0), 0.0);
    assert_eq!(math::wrapi(-1, 0, 3), 2);
    assert!(approx(math::pingpong(2.5, 2.0), 1.5));
    assert!(approx(math::pingpong(-1.0, 2.0), 1.0));
    assert_eq!(math::move_toward(1.0, 10.0, 3.0), 4.0);
    assert_eq!(math::move_toward(10.0, 1.0, 20.0), 1.0);
    assert_eq!(math::smoothstep(0.0, 1.0, 0.5), 0.5);
    assert_eq!(math::smoothstep(1.0, 1.0, 0.5), 0.0);
    assert_eq!(math::smoothstep(1.0, 1.0, 1.5), 1.0);
    assert!(approx(math::remap(5.0, 0.0, 10.0, 100.0, 200.0), 150.0));
    assert!(approx(math::inverse_lerp(10.0, 20.0, 15.0), 0.5));
    assert!(approx(math::lerp_angle(0.1, (TAU - 0.1) as Real, 0.5), 0.0));
    assert!((math::ease(0.5, 2.0) - 0.25).abs() < 1e-9);
    assert!((math::ease(0.5, -2.0) - 0.5).abs() < 1e-9);
    assert!(math::is_equal_approx(1.0, 1.000001));
    assert!(!math::is_equal_approx(1.0, 1.0001));
    assert!(math::is_equal_approx(f32::INFINITY, f32::INFINITY));
    assert_eq!(math::string_num(0.0, 4), "0.0");
    assert_eq!(math::string_num(1.23456789, 4), "1.2346");
    assert_eq!(math::string_num_real(100.0, true), "100.0");
    assert_eq!(math::string_num_real(-2.5, false), "-2.5");
}
