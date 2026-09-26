//! Port of the parts of libs/librepcb/rust-core/src/math.rs used by the
//! basic types (angle conversion and point rotation).
//!
//! All functions are deterministic across platforms because they use the
//! floating point implementation of the [`libm`] crate, exactly like upstream.

/// π/180 as a deterministic constant (bit-identical to upstream).
const PI_DIV_180: f64 = f64::from_bits(0x3F91DF46A2529D39);

/// Converts degrees to radians.
pub fn to_radians(degrees: f64) -> f64 {
    degrees * PI_DIV_180
}

/// Converts radians to degrees.
pub fn to_degrees(radians: f64) -> f64 {
    radians / PI_DIV_180
}

/// Rotates the point `(x, y)` by `angle` degrees (counter-clockwise) around
/// the origin.
///
/// This always uses floating point math; callers should handle multiples of
/// 90° explicitly. `angle` is expected to be in the range ]-360..360[.
pub fn rotate_point(x: f64, y: f64, angle: f64) -> (f64, f64) {
    debug_assert!((angle > -360.0) && (angle < 360.0));
    let radians = to_radians(angle);
    let sin = libm::sin(radians);
    let cos = libm::cos(radians);
    (cos * x - sin * y, sin * x + cos * y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_radians_roundtrip() {
        assert_eq!(to_degrees(std::f64::consts::PI), 180.0);
        assert_eq!(to_radians(180.0), std::f64::consts::PI);
    }

    #[test]
    fn rotate() {
        let (x, y) = rotate_point(10.0, 0.0, 90.0);
        assert!(x.abs() < 1e-12);
        assert!((y - 10.0).abs() < 1e-12);
    }
}
