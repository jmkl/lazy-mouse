//! Catenary rope between two points (the hanging rope seen in the lazy-brush demo).
//!
//! Standard catenary math, solved numerically. `y` grows downward (screen
//! coordinates), so the rope sags toward larger `y`.

use crate::brush::Point;

/// Fill `out` with evenly spaced (in x) points of a rope of `length` hanging
/// from `a` to `b`. If the rope is not longer than the distance between the
/// points, it is a straight line.
pub fn sample(a: Point, b: Point, length: f64, out: &mut [Point]) {
    let n = out.len();
    if n == 0 {
        return;
    }
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dist = dx.hypot(dy);

    // Taut (or nearly): straight line.
    if n < 2 || length - dist < 1e-3 {
        for (i, p) in out.iter_mut().enumerate() {
            let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            *p = Point { x: a.x + dx * t, y: a.y + dy * t };
        }
        return;
    }

    // Work in local coordinates: s runs horizontally from `a` toward `b`.
    // A (near) vertical rope is widened to 0.5px so the math stays finite;
    // it then draws as a thin "V" hanging down.
    let w = dx.abs().max(0.5);
    let sgn = if dx < 0.0 { -1.0 } else { 1.0 };

    // Solve  sqrt(L^2 - dy^2) = 2a * sinh(w / 2a)  for `a`.
    // With u = w / 2a this is sinh(u) / u = r, which is increasing in u.
    let r = (length * length - dy * dy).max(0.0).sqrt() / w;
    let u = solve_sinhc(r);
    let a_par = w / (2.0 * u);

    // Horizontal position of the lowest point, then the vertical offset.
    let x0 = w / 2.0 + a_par * (dy / (2.0 * a_par * (w / (2.0 * a_par)).sinh())).asinh();
    let c = a.y + a_par * (x0 / a_par).cosh();

    for (i, p) in out.iter_mut().enumerate() {
        let s = w * i as f64 / (n - 1) as f64;
        *p = Point {
            x: a.x + sgn * s,
            y: c - a_par * ((s - x0) / a_par).cosh(),
        };
    }
}

/// Solve sinh(u)/u = r for u > 0 (r > 1) by bisection.
fn solve_sinhc(r: f64) -> f64 {
    let f = |u: f64| u.sinh() / u;
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    // sinh overflows near 710, so cap the search.
    while f(hi) < r && hi < 700.0 {
        lo = hi;
        hi *= 2.0;
    }
    hi = hi.min(700.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if mid <= 0.0 {
            break;
        }
        if f(mid) < r {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi)).max(1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: Point = Point { x: 0.0, y: 0.0 };

    fn poly_len(p: &[Point]) -> f64 {
        p.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum()
    }

    #[test]
    fn taut_rope_is_a_straight_line() {
        let mut out = [Z; 5];
        sample(Z, Point { x: 40.0, y: 0.0 }, 40.0, &mut out);
        assert_eq!(out[2], Point { x: 20.0, y: 0.0 });
    }

    #[test]
    fn slack_rope_hangs_down_and_has_the_requested_length() {
        let mut out = [Z; 256];
        let b = Point { x: 30.0, y: 10.0 };
        sample(Z, b, 50.0, &mut out);
        assert!((out[0].x).abs() < 1e-9 && (out[0].y).abs() < 1e-9);
        assert!((out[255].x - b.x).abs() < 1e-9 && (out[255].y - b.y).abs() < 1e-6);
        // below the straight chord at the middle (y grows downward)
        assert!(out[128].y > 5.0 + 1.0);
        assert!((poly_len(&out) - 50.0).abs() < 0.05, "len {}", poly_len(&out));
    }

    #[test]
    fn works_right_to_left_and_vertical() {
        let mut out = [Z; 64];
        sample(Point { x: 100.0, y: 100.0 }, Point { x: 70.0, y: 100.0 }, 45.0, &mut out);
        assert!((poly_len(&out) - 45.0).abs() < 0.2);
        sample(Z, Point { x: 0.0, y: 20.0 }, 30.0, &mut out);
        assert!(out.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    }

    #[test]
    fn always_finite() {
        let mut out = [Z; 32];
        let mut s = 12345u64;
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as f64 / (1u64 << 31) as f64
        };
        for _ in 0..20_000 {
            let a = Point { x: rnd() * 200.0, y: rnd() * 200.0 };
            let b = Point { x: rnd() * 200.0, y: rnd() * 200.0 };
            sample(a, b, rnd() * 300.0, &mut out);
            assert!(out.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }
}
