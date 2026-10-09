//! The lazy-brush math. Pure, no OS calls, so it is testable on any platform.
//!
//! The brush only moves when the pointer leaves a circle of `radius` around it.
//! When it does, it moves just far enough along the line to the pointer that the
//! pointer sits exactly on the circle again (`distance - radius`).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LazyBrush {
    radius: f64,
    /// 0 = brush closes the whole gap, 1 = brush never moves.
    friction: f64,
    brush: Point,
}

impl LazyBrush {
    pub const fn new(radius: f64, initial: Point) -> Self {
        Self {
            radius,
            friction: 0.0,
            brush: initial,
        }
    }

    /// Fraction (0..=1) of the gap the brush does NOT close per update.
    pub fn with_friction(mut self, friction: f64) -> Self {
        self.friction = if friction < 0.0 {
            0.0
        } else if friction > 1.0 {
            1.0
        } else {
            friction
        };
        self
    }
    /// Change the lazy radius on the fly (the brush position is kept).
    pub fn set_radius(&mut self, radius: f64) {
        self.radius = radius.max(0.0);
    }

    /// Change the friction on the fly (clamped to 0..=1).
    pub fn set_friction(&mut self, friction: f64) {
        *self = self.with_friction(friction);
    }

    pub const fn radius(&self) -> f64 {
        self.radius
    }

    pub const fn brush(&self) -> Point {
        self.brush
    }

    /// Feed a new pointer position. Returns `true` if the brush moved.
    #[inline]
    pub fn update(&mut self, pointer: Point) -> bool {
        let dx = pointer.x - self.brush.x;
        let dy = pointer.y - self.brush.y;
        let dist_sq = dx * dx + dy * dy;
        // Inside the lazy area: no move, and no sqrt.
        if dist_sq <= self.radius * self.radius {
            return false;
        }
        if self.friction >= 1.0 {
            return false;
        }
        let dist = dist_sq.sqrt();
        let k = (dist - self.radius) / dist * (1.0 - self.friction);
        self.brush.x += dx * k;
        self.brush.y += dy * k;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

    #[test]
    fn inside_radius_does_not_move() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        assert!(!b.update(Point { x: 20.0, y: 0.0 }));
        assert_eq!(b.brush(), ORIGIN);
    }

    #[test]
    fn matches_lazy_brush_readme_example() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        assert!(b.update(Point { x: 40.0, y: 0.0 }));
        assert_eq!(b.brush(), Point { x: 10.0, y: 0.0 });
    }

    #[test]
    fn pointer_ends_exactly_on_the_radius() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        let p = Point { x: 100.0, y: 75.0 };
        assert!(b.update(p));
        let d = ((p.x - b.brush().x).powi(2) + (p.y - b.brush().y).powi(2)).sqrt();
        assert!((d - 30.0).abs() < 1e-9);
    }

    #[test]
    fn setters_apply_to_next_update() {
        let mut b = LazyBrush::new(30.0, ORIGIN);
        b.set_radius(10.0);
        assert!(b.update(Point { x: 40.0, y: 0.0 }));
        assert_eq!(b.brush(), Point { x: 30.0, y: 0.0 });
        b.set_friction(1.0);
        assert!(!b.update(Point { x: 400.0, y: 0.0 }));
    }

    #[test]
    fn friction_slows_the_brush() {
        // lazy-brush README: friction reduces the speed; 0 = none, 1 = never moves
        let mut b = LazyBrush::new(30.0, ORIGIN).with_friction(0.5);
        assert!(b.update(Point { x: 40.0, y: 0.0 }));
        assert_eq!(b.brush(), Point { x: 5.0, y: 0.0 });
        let mut stuck = LazyBrush::new(30.0, ORIGIN).with_friction(1.0);
        assert!(!stuck.update(Point { x: 400.0, y: 0.0 }));
        assert_eq!(stuck.brush(), ORIGIN);
    }

    #[test]
    fn zero_radius_follows_pointer() {
        let mut b = LazyBrush::new(0.0, ORIGIN);
        let p = Point { x: 7.0, y: -3.0 };
        assert!(b.update(p));
        assert_eq!(b.brush(), p);
        assert!(!b.update(p));
    }
}
