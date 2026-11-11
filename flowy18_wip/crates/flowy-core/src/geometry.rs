//! Geometry primitives shared by the editor and the canvas renderer.
//!
//! Coordinates in the document are "world" units. The canvas converts them to
//! screen units through [`Camera`]. Keeping the conversion here, away from
//! gpui, makes the viewport behavior ordinary logic that can be tested.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// The smallest canvas zoom the editor allows.
pub const MIN_ZOOM: f32 = 0.2;
/// The largest canvas zoom the editor allows.
pub const MAX_ZOOM: f32 = 3.0;

/// A point or vector in two dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Creates a vector from its components.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns both components as a `(x, y)` tuple.
    #[must_use]
    pub const fn to_tuple(self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Returns the squared length, avoiding a square root.
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Returns the Euclidean length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns the distance to `other`.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    /// Returns a vector rounded to the nearest multiple of `grid`.
    #[must_use]
    pub fn snap(self, grid: f32) -> Self {
        if grid <= 0.0 {
            return self;
        }
        Self::new(
            (self.x / grid).round() * grid,
            (self.y / grid).round() * grid,
        )
    }

    /// Returns a vector moved toward `target` by at most `max_delta`.
    #[must_use]
    pub fn move_toward(self, target: Self, max_delta: f32) -> Self {
        let delta = target - self;
        let distance = delta.length();
        if distance <= max_delta || distance == 0.0 {
            target
        } else {
            self + delta * (max_delta / distance)
        }
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Vec2 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

/// An axis-aligned rectangle in world coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Creates a rectangle from its origin and size.
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Returns the rectangle spanning two arbitrary corners.
    #[must_use]
    pub fn from_points(a: Vec2, b: Vec2) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self::new(x, y, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    /// Returns the top-left corner.
    #[must_use]
    pub const fn origin(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Returns the width and height.
    #[must_use]
    pub const fn size(self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    /// Returns the right edge.
    #[must_use]
    pub const fn right(self) -> f32 {
        self.x + self.w
    }

    /// Returns the bottom edge.
    #[must_use]
    pub const fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Returns the center.
    #[must_use]
    pub const fn center(self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Returns whether the rectangle has no area.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Returns whether `point` lies inside the rectangle, edges included.
    #[must_use]
    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// Returns whether two rectangles overlap. Touching edges do not count.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Returns the smallest rectangle containing both inputs.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }

    /// Returns a rectangle shrunk by `amount` on every side.
    #[must_use]
    pub fn inset(self, amount: f32) -> Self {
        Self::new(
            self.x + amount,
            self.y + amount,
            (self.w - amount * 2.0).max(0.0),
            (self.h - amount * 2.0).max(0.0),
        )
    }

    /// Returns a rectangle expanded by `amount` on every side.
    #[must_use]
    pub fn expand(self, amount: f32) -> Self {
        Self::new(
            self.x - amount,
            self.y - amount,
            self.w + amount * 2.0,
            self.h + amount * 2.0,
        )
    }

    /// Returns a rectangle moved by `delta`.
    #[must_use]
    pub fn translated(self, delta: Vec2) -> Self {
        Self::new(self.x + delta.x, self.y + delta.y, self.w, self.h)
    }
}

/// The viewport transform: a world-space center and a zoom factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// World point displayed at the center of the canvas.
    pub center: Vec2,
    /// Pixels per world unit.
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// Converts a world point to canvas-local screen pixels.
    #[must_use]
    pub fn world_to_screen(self, world: Vec2, viewport: Vec2) -> Vec2 {
        (world - self.center) * self.zoom + viewport / 2.0
    }

    /// Converts canvas-local screen pixels to a world point.
    #[must_use]
    pub fn screen_to_world(self, screen: Vec2, viewport: Vec2) -> Vec2 {
        self.center + (screen - viewport / 2.0) / self.zoom
    }

    /// Moves the content by a screen-space delta, as when dragging with the
    /// hand tool.
    pub fn pan_by(&mut self, screen_delta: Vec2) {
        self.center -= screen_delta / self.zoom;
    }

    /// Zooms by `factor`, keeping the world point under `screen` fixed.
    pub fn zoom_at(&mut self, factor: f32, screen: Vec2, viewport: Vec2) {
        let anchor = self.screen_to_world(screen, viewport);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = anchor - (screen - viewport / 2.0) / self.zoom;
    }

    /// Sets an absolute zoom, keeping the world point under `screen` fixed.
    pub fn set_zoom_at(&mut self, zoom: f32, screen: Vec2, viewport: Vec2) {
        let target = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if self.zoom == 0.0 {
            self.zoom = target;
            return;
        }
        self.zoom_at(target / self.zoom, screen, viewport);
    }

    /// Frames `content` in a viewport, leaving `padding` screen pixels around
    /// it. Empty content centers on its origin at 100% zoom.
    #[must_use]
    pub fn fit(content: Rect, viewport: Vec2, padding: f32) -> Self {
        if content.is_empty() || viewport.x <= 0.0 || viewport.y <= 0.0 {
            return Self {
                center: content.center(),
                zoom: 1.0,
            };
        }

        let available = Vec2::new(
            (viewport.x - padding * 2.0).max(1.0),
            (viewport.y - padding * 2.0).max(1.0),
        );
        let zoom = (available.x / content.w)
            .min(available.y / content.h)
            .clamp(MIN_ZOOM, MAX_ZOOM);

        Self {
            center: content.center(),
            zoom,
        }
    }

    /// Returns the world rectangle currently visible inside `viewport`.
    #[must_use]
    pub fn visible_rect(self, viewport: Vec2) -> Rect {
        let half = viewport / (2.0 * self.zoom);
        Rect::new(
            self.center.x - half.x,
            self.center.y - half.y,
            half.x * 2.0,
            half.y * 2.0,
        )
    }
}

/// The four control points of the cubic curve drawn for a connection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeGeometry {
    /// The source port.
    pub start: Vec2,
    /// The first control point.
    pub control_a: Vec2,
    /// The second control point.
    pub control_b: Vec2,
    /// The destination port.
    pub end: Vec2,
}

impl EdgeGeometry {
    /// Builds a horizontal S-curve between two ports.
    #[must_use]
    pub fn between(start: Vec2, end: Vec2) -> Self {
        let dx = (end.x - start.x).abs();
        let handle = (dx * 0.5).max(56.0);
        Self {
            start,
            control_a: Vec2::new(start.x + handle, start.y),
            control_b: Vec2::new(end.x - handle, end.y),
            end,
        }
    }

    /// Returns the point at `t` in `0..=1` along the curve.
    #[must_use]
    pub fn point_at(self, t: f32) -> Vec2 {
        let t = t.clamp(0.0, 1.0);
        let mt = 1.0 - t;
        let a = mt * mt * mt;
        let b = 3.0 * mt * mt * t;
        let c = 3.0 * mt * t * t;
        let d = t * t * t;
        Vec2::new(
            a * self.start.x
                + b * self.control_a.x
                + c * self.control_b.x
                + d * self.end.x,
            a * self.start.y
                + b * self.control_a.y
                + c * self.control_b.y
                + d * self.end.y,
        )
    }

    /// Returns the tangent direction at `t`, normalized when possible.
    #[must_use]
    pub fn tangent_at(self, t: f32) -> Vec2 {
        let t = t.clamp(0.0, 1.0);
        let mt = 1.0 - t;
        let a = 3.0 * mt * mt;
        let b = 6.0 * mt * t;
        let c = 3.0 * t * t;
        let tangent = Vec2::new(
            a * (self.control_a.x - self.start.x)
                + b * (self.control_b.x - self.control_a.x)
                + c * (self.end.x - self.control_b.x),
            a * (self.control_a.y - self.start.y)
                + b * (self.control_b.y - self.control_a.y)
                + c * (self.end.y - self.control_b.y),
        );
        let length = tangent.length();
        if length <= f32::EPSILON {
            Vec2::new(1.0, 0.0)
        } else {
            tangent / length
        }
    }

    /// Returns the smallest distance from `point` to the curve.
    #[must_use]
    pub fn distance_to(self, point: Vec2) -> f32 {
        const SAMPLES: usize = 32;
        let mut previous = self.start;
        let mut closest = point.distance(previous);
        for step in 1..=SAMPLES {
            let next = self.point_at(step as f32 / SAMPLES as f32);
            closest = closest.min(distance_to_segment(point, previous, next));
            previous = next;
        }
        closest
    }
}

/// Returns the distance from `point` to the segment `a..b`.
#[must_use]
pub fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).x * ab.x + (point - a).y * ab.y) / length_squared;
    let projection = a + ab * t.clamp(0.0, 1.0);
    point.distance(projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn rectangles_intersect_only_when_they_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.intersects(Rect::new(5.0, 5.0, 10.0, 10.0)));
        assert!(!a.intersects(Rect::new(10.0, 0.0, 5.0, 5.0)));
        assert!(a.contains(Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn union_covers_both_rectangles() {
        let union = Rect::new(0.0, 0.0, 10.0, 10.0)
            .union(Rect::new(20.0, -5.0, 5.0, 5.0));
        assert_eq!(union, Rect::new(0.0, -5.0, 25.0, 15.0));
    }

    #[test]
    fn camera_round_trips_screen_and_world() {
        let camera = Camera {
            center: Vec2::new(100.0, -40.0),
            zoom: 1.75,
        };
        let viewport = Vec2::new(900.0, 600.0);
        let world = Vec2::new(310.0, 88.0);
        let screen = camera.world_to_screen(world, viewport);
        let round_trip = camera.screen_to_world(screen, viewport);
        assert!(close(world.x, round_trip.x));
        assert!(close(world.y, round_trip.y));
    }

    #[test]
    fn zooming_keeps_the_anchor_point_fixed() {
        let mut camera = Camera {
            center: Vec2::ZERO,
            zoom: 1.0,
        };
        let viewport = Vec2::new(800.0, 600.0);
        let screen = Vec2::new(620.0, 140.0);
        let before = camera.screen_to_world(screen, viewport);
        camera.zoom_at(1.8, screen, viewport);
        let after = camera.screen_to_world(screen, viewport);
        assert!(close(before.x, after.x));
        assert!(close(before.y, after.y));
    }

    #[test]
    fn fit_centers_and_scales_content() {
        let content = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let camera = Camera::fit(content, Vec2::new(500.0, 500.0), 40.0);
        assert_eq!(camera.center, Vec2::new(500.0, 250.0));
        assert!(close(camera.zoom, 0.42));
    }

    #[test]
    fn cubic_curve_hit_test_follows_the_curve() {
        let edge =
            EdgeGeometry::between(Vec2::new(0.0, 0.0), Vec2::new(400.0, 200.0));
        for step in 0..=10 {
            let point = edge.point_at(step as f32 / 10.0);
            assert!(edge.distance_to(point) < 0.01);
        }
        assert!(edge.distance_to(Vec2::new(200.0, -200.0)) > 50.0);
    }

    #[test]
    fn snapping_rounds_to_the_grid() {
        assert_eq!(
            Vec2::new(13.2, -7.4).snap(8.0),
            Vec2::new(16.0, -8.0),
            "snap should use the nearest grid point"
        );
    }
}
