//! The lazy-brush math. Pure, no OS calls, so it is testable on any platform.
//!
//! The brush only moves while the pointer is outside a circle of `radius` around
//! it. Outside, the pointer pulls the brush like a rubber band: the force grows
//! faster than linearly with the stretch. The moment the pointer is back inside
//! the circle, the brush stops dead.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };
}

/// Rubber-band feel. Values are kept in stable ranges by `config` (the fixed
/// 4 ms integration step blows up with extreme values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Higher = snappier pull.
    pub stiffness: f64,
    /// Lower = builds speed more freely.
    pub damping: f64,
    /// Lower = more nonlinear (very large = plain linear spring).
    pub stretch_ref: f64,
}

impl Spring {
    pub const DEFAULT: Spring = Spring {
        stiffness: 180.0,
        damping: 22.0,
        stretch_ref: 100.0,
    };
}

/// Integration substep and the largest frame time we simulate (seconds).
const SUBSTEP: f64 = 0.004;
const MAX_DT: f64 = 0.05;
/// Friction scales the spring by `(1 - friction)^FRICTION_CURVE`. 1 = linear (subtle);
/// 3 makes small values like 0.1 clearly noticeable (0.1 -> 73% strength, 0.5 -> 12.5%).
const FRICTION_CURVE: i32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct LazyBrush {
    radius: f64,
    /// 0 = full spring strength, 1 = brush never moves.
    friction: f64,
    brush: Point,
    vel: Point,
    spring: Spring,
}

impl LazyBrush {
    pub const fn new(radius: f64, initial: Point) -> Self {
        Self {
            radius,
            friction: 0.0,
            brush: initial,
            vel: Point::ZERO,
            spring: Spring::DEFAULT,
        }
    }

    pub fn with_friction(mut self, friction: f64) -> Self {
        self.set_friction(friction);
        self
    }

    pub fn with_spring(mut self, spring: Spring) -> Self {
        self.spring = spring;
        self
    }

    /// Change the lazy radius on the fly (the brush position is kept).
    pub fn set_radius(&mut self, radius: f64) {
        self.radius = radius.max(0.0);
    }

    /// Change the friction on the fly (clamped to 0..=1).
    pub fn set_friction(&mut self, friction: f64) {
        self.friction = friction.clamp(0.0, 1.0);
    }

    pub fn set_spring(&mut self, spring: Spring) {
        self.spring = spring;
    }

    pub const fn radius(&self) -> f64 {
        self.radius
    }

    pub const fn brush(&self) -> Point {
        self.brush
    }

    /// Advance the simulation by `dt` seconds towards `pointer`.
    /// Returns `true` if the brush position changed.
    pub fn step(&mut self, pointer: Point, dt: f64) -> bool {
        if self.friction >= 1.0 {
            self.vel = Point::ZERO;
            return false;
        }
        let dt = dt.clamp(0.0, MAX_DT);
        let n = (dt / SUBSTEP).ceil() as u32;
        if n == 0 {
            return false;
        }
        let h = dt / f64::from(n);
        let Spring {
            stiffness,
            damping,
            stretch_ref,
        } = self.spring;
        let k = stiffness * (1.0 - self.friction).powi(FRICTION_CURVE);
        let r2 = self.radius * self.radius;
        let before = self.brush;

        for _ in 0..n {
            let dx = pointer.x - self.brush.x;
            let dy = pointer.y - self.brush.y;
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 {
                break; // inside the lazy area: no pull
            }
            let dist = d2.sqrt();
            let excess = dist - self.radius;
            let f = k * excess * (1.0 + excess / stretch_ref) / dist;
            self.vel.x += (dx * f - damping * self.vel.x) * h;
            self.vel.y += (dy * f - damping * self.vel.y) * h;
            self.brush.x += self.vel.x * h;
            self.brush.y += self.vel.y * h;
        }

        // Inside the radius (at the start or reached during this step): stop instantly.
        let dx = pointer.x - self.brush.x;
        let dy = pointer.y - self.brush.y;
        if dx * dx + dy * dy <= r2 {
            self.vel = Point::ZERO;
        }
        self.brush != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Point = Point::ZERO;
    const FRAME: f64 = 0.016;

    fn at_x(x: f64) -> Point {
        Point { x, y: 0.0 }
    }

    #[test]
    fn inside_radius_does_not_move() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        assert!(!b.step(at_x(20.0), FRAME));
        assert_eq!(b.brush(), ORIGIN);
    }

    #[test]
    fn pulls_towards_the_pointer_along_the_line() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        assert!(b.step(at_x(100.0), FRAME));
        let q = b.brush();
        assert!(q.x > 0.0 && q.x < 100.0);
        assert_eq!(q.y, 0.0);
    }

    #[test]
    fn stops_dead_when_the_pointer_is_back_inside() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        for _ in 0..10 {
            b.step(at_x(200.0), FRAME);
        }
        assert!(b.vel.x > 0.0, "brush should be moving");
        let held = b.brush();
        assert!(!b.step(at_x(held.x + 10.0), FRAME));
        assert_eq!(b.brush(), held);
        assert_eq!(b.vel, Point::ZERO);
    }

    #[test]
    fn settles_with_the_pointer_inside_the_radius() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        let p = Point { x: 100.0, y: 75.0 };
        let mut frames = 0;
        while b.step(p, FRAME) {
            frames += 1;
            assert!(frames < 600, "never settled");
        }
        let d = (p.x - b.brush().x).hypot(p.y - b.brush().y);
        assert!(d <= 30.0 + 1e-9, "d = {d}");
        assert_eq!(b.vel, Point::ZERO);
    }

    #[test]
    fn friction_one_never_moves() {
        let mut b = LazyBrush::new(30.0, ORIGIN).with_friction(1.0);
        assert!(!b.step(at_x(400.0), FRAME));
        assert_eq!(b.brush(), ORIGIN);
    }

    #[test]
    fn friction_weakens_the_pull() {
        let mut free = LazyBrush::new(30.0, ORIGIN);
        let mut slow = LazyBrush::new(30.0, ORIGIN).with_friction(0.5);
        for _ in 0..10 {
            free.step(at_x(300.0), FRAME);
            slow.step(at_x(300.0), FRAME);
        }
        assert!(slow.brush().x > 0.0 && slow.brush().x < free.brush().x);
    }

    #[test]
    fn small_friction_is_clearly_noticeable() {
        let mut free = LazyBrush::new(30.0, ORIGIN);
        let mut light = LazyBrush::new(30.0, ORIGIN).with_friction(0.1);
        for _ in 0..10 {
            free.step(at_x(300.0), FRAME);
            light.step(at_x(300.0), FRAME);
        }
        assert!(light.brush().x < 0.9 * free.brush().x);
    }

    #[test]
    fn smaller_stretch_ref_pulls_harder() {
        let soft = Spring {
            stretch_ref: 1.0e6,
            ..Spring::DEFAULT
        };
        let hard = Spring {
            stretch_ref: 30.0,
            ..Spring::DEFAULT
        };
        let mut a = LazyBrush::new(30.0, ORIGIN).with_spring(soft);
        let mut b = LazyBrush::new(30.0, ORIGIN).with_spring(hard);
        for _ in 0..3 {
            a.step(at_x(200.0), FRAME);
            b.step(at_x(200.0), FRAME);
        }
        assert!(b.brush().x > a.brush().x);
    }

    #[test]
    fn setters_apply_to_next_step() {
        let mut b = LazyBrush::new(50.0, ORIGIN);
        assert!(!b.step(at_x(40.0), FRAME));
        b.set_radius(10.0);
        assert!(b.step(at_x(40.0), FRAME));
        b.set_friction(1.0);
        assert!(!b.step(at_x(400.0), FRAME));
    }
}
