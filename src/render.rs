//! Software renderer for the rope overlay (tiny-skia, anti-aliased).
//! Pure: it only fills a pixmap, so it works and tests on any platform.

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::brush::Point;
use crate::rope;

/// Thickness of the dark line, in px.
const ROPE_WIDTH: f32 = 2.0;
/// Extra thickness of the light outline under it (total outline = ROPE_WIDTH + this).
const OUTLINE_EXTRA: f32 = 1.0;
/// Dash pattern in px: visible length, then gap. Set `DASH_ON` to 0 for a solid line.
const DASH_ON: f32 = 6.0;
const DASH_OFF: f32 = 5.0;
/// Radius of the red pointer dot (the white ring is 1.5px wider).
const DOT_RADIUS: f32 = 6.0;

/// Empty border around the drawing so strokes and the dot are never cut off.
const MARGIN: u32 = 12;
const SAMPLES: usize = 32;

/// Square canvas centred on the brush, reaching `2 * radius` in every direction.
/// The pointer can lag further than that while the brush is catching up, so the
/// drawn end is clamped to the canvas.
pub struct Renderer {
    pixmap: Pixmap,
    side: u32,
    /// Built once (it owns the dash vector); only `width` changes per pass.
    stroke: Stroke,
}

impl Renderer {
    pub fn new(radius: f64) -> Option<Self> {
        let mut side = (radius * 4.0).ceil() as u32 + 2 * MARGIN;
        side += side & 1; // even, so the centre lands on a whole pixel
        let dash = if DASH_ON > 0.0 {
            StrokeDash::new(vec![DASH_ON, DASH_OFF], 0.0)
        } else {
            None
        };
        // round caps look nice on a solid line, but would blur the gaps of a dashed one
        let stroke = Stroke {
            line_cap: if dash.is_some() {
                LineCap::Butt
            } else {
                LineCap::Round
            },
            line_join: LineJoin::Round,
            dash,
            ..Stroke::default()
        };
        Some(Self {
            pixmap: Pixmap::new(side, side)?,
            side,
            stroke,
        })
    }

    pub fn side(&self) -> u32 {
        self.side
    }

    /// Premultiplied RGBA bytes, row-major, `side * side * 4` of them.
    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// `brush` is the (integer-rounded) cursor position in screen coordinates,
    /// `pointer` the real pointer. Both are mapped into canvas space.
    pub fn draw(&mut self, brush: Point, pointer: Point, length: f64) {
        self.pixmap.fill(Color::TRANSPARENT);

        let half = f64::from(self.side) / 2.0;
        let a = Point { x: half, y: half };
        let (mut vx, mut vy) = (pointer.x - brush.x, pointer.y - brush.y);
        let reach = half - f64::from(MARGIN);
        let d = vx.hypot(vy);
        if d > reach {
            let k = reach / d;
            vx *= k;
            vy *= k;
        }
        let b = Point {
            x: vx + half,
            y: vy + half,
        };

        let mut pts = [a; SAMPLES];
        rope::sample(a, b, length, &mut pts);

        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].x as f32, pts[0].y as f32);
        for p in &pts[1..] {
            pb.line_to(p.x as f32, p.y as f32);
        }

        let mut paint = Paint::default();
        paint.anti_alias = true;

        if let Some(path) = pb.finish() {
            // light outline under a dark core, so it reads on any background
            paint.set_color_rgba8(255, 255, 255, 200);
            self.stroke.width = ROPE_WIDTH + OUTLINE_EXTRA;
            self.pixmap
                .stroke_path(&path, &paint, &self.stroke, Transform::identity(), None);
            paint.set_color_rgba8(20, 20, 20, 235);
            self.stroke.width = ROPE_WIDTH;
            self.pixmap
                .stroke_path(&path, &paint, &self.stroke, Transform::identity(), None);
        }

        // pointer end
        if let Some(dot) = PathBuilder::from_circle(b.x as f32, b.y as f32, DOT_RADIUS + 1.5) {
            paint.set_color_rgba8(255, 255, 255, 220);
            self.pixmap
                .fill_path(&dot, &paint, FillRule::Winding, Transform::identity(), None);
            if let Some(core) = PathBuilder::from_circle(b.x as f32, b.y as f32, DOT_RADIUS) {
                paint.set_color_rgba8(230, 60, 60, 255);
                self.pixmap.fill_path(
                    &core,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }

    #[cfg(test)]
    pub fn save_png(&self, path: &str) {
        self.pixmap.save_png(path).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(r: &Renderer) -> impl Iterator<Item = u8> + '_ {
        r.data().chunks_exact(4).map(|p| p[3])
    }

    #[test]
    fn draws_something_and_clears_between_frames() {
        let mut r = Renderer::new(40.0).unwrap();
        let brush = Point { x: 500.0, y: 500.0 };
        r.draw(brush, Point { x: 520.0, y: 510.0 }, 40.0);
        assert!(alpha(&r).any(|a| a > 0));
        // everything stays inside the canvas margin: border row untouched
        let s = r.side() as usize;
        assert!(alpha(&r).take(s).all(|a| a == 0));
        r.pixmap.fill(Color::TRANSPARENT);
        assert!(alpha(&r).all(|a| a == 0));
    }

    #[test]
    fn far_pointer_is_clamped_inside_the_canvas() {
        let mut r = Renderer::new(40.0).unwrap();
        r.draw(Point { x: 0.0, y: 0.0 }, Point { x: 5000.0, y: 5000.0 }, 40.0);
        let s = r.side() as usize;
        assert!(alpha(&r).take(s).all(|a| a == 0));
        assert!(alpha(&r).any(|a| a > 0));
    }

    #[test]
    #[ignore]
    fn write_preview_png() {
        let mut r = Renderer::new(80.0).unwrap();
        r.draw(Point { x: 0.0, y: 0.0 }, Point { x: 60.0, y: -25.0 }, 80.0);
        r.save_png("/tmp/rope_preview.png");
    }
}
