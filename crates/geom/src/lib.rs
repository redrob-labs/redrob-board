//! Integer geometry on nanometre coordinates: points, rectangles, segments and simple polygons.
//!
//! Everything here is exact. Distances are compared squared, as `i128`, so no square root and no
//! float ever decides a DRC verdict.

#![forbid(unsafe_code)]

use board_units::Nm;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: Nm,
    pub y: Nm,
}

impl Point {
    pub const ORIGIN: Point = Point { x: Nm::ZERO, y: Nm::ZERO };

    pub const fn new(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    /// Both coordinates in exact millimetres.
    pub const fn mm(x: i64, y: i64) -> Point {
        Point { x: Nm::mm(x), y: Nm::mm(y) }
    }

    pub fn offset(self, dx: Nm, dy: Nm) -> Point {
        Point { x: self.x + dx, y: self.y + dy }
    }

    pub fn translate(self, by: Point) -> Point {
        self.offset(by.x, by.y)
    }

    pub fn squared_distance(self, other: Point) -> i128 {
        (self.x - other.x).squared() + (self.y - other.y).squared()
    }

    /// Rotate about the origin by a multiple of 90 degrees, counter-clockwise, y up.
    pub fn rotate_quarter_turns(self, turns: u8) -> Point {
        match turns % 4 {
            0 => self,
            1 => Point { x: -self.y, y: self.x },
            2 => Point { x: -self.x, y: -self.y },
            _ => Point { x: self.y, y: -self.x },
        }
    }
}

/// Axis-aligned rectangle, `min` inclusive, `max` inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub fn from_points(a: Point, b: Point) -> Rect {
        Rect { min: Point::new(a.x.min(b.x), a.y.min(b.y)), max: Point::new(a.x.max(b.x), a.y.max(b.y)) }
    }

    pub fn centered(center: Point, width: Nm, height: Nm) -> Rect {
        let half_w = width / 2;
        let half_h = height / 2;
        Rect { min: center.offset(-half_w, -half_h), max: center.offset(half_w, half_h) }
    }

    pub fn width(&self) -> Nm {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> Nm {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        Point::new((self.min.x + self.max.x) / 2, (self.min.y + self.max.y) / 2)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x <= other.max.x && other.min.x <= self.max.x && self.min.y <= other.max.y && other.min.y <= self.max.y
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            min: Point::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Point::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// Grow every side by `by` (shrink when negative).
    pub fn inflate(&self, by: Nm) -> Rect {
        Rect { min: self.min.offset(-by, -by), max: self.max.offset(by, by) }
    }

    /// Smallest gap between two rectangles, squared; zero when they touch or overlap.
    pub fn squared_gap(&self, other: &Rect) -> i128 {
        let dx = (other.min.x - self.max.x).max(self.min.x - other.max.x).max(Nm::ZERO);
        let dy = (other.min.y - self.max.y).max(self.min.y - other.max.y).max(Nm::ZERO);
        dx.squared() + dy.squared()
    }

    pub fn corners(&self) -> [Point; 4] {
        [self.min, Point::new(self.max.x, self.min.y), self.max, Point::new(self.min.x, self.max.y)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

impl Segment {
    pub const fn new(a: Point, b: Point) -> Segment {
        Segment { a, b }
    }

    pub fn bbox(&self) -> Rect {
        Rect::from_points(self.a, self.b)
    }

    pub fn squared_length(&self) -> i128 {
        self.a.squared_distance(self.b)
    }

    /// Length rounded to the nearest nanometre.
    pub fn length(&self) -> Nm {
        Nm((self.squared_length() as f64).sqrt().round() as i64)
    }

    /// Squared distance from `p` to the closest point of the segment.
    pub fn squared_distance_to_point(&self, p: Point) -> i128 {
        let ax = self.a.x.0 as i128;
        let ay = self.a.y.0 as i128;
        let dx = (self.b.x - self.a.x).0 as i128;
        let dy = (self.b.y - self.a.y).0 as i128;
        let px = p.x.0 as i128;
        let py = p.y.0 as i128;
        let len2 = dx * dx + dy * dy;
        if len2 == 0 {
            return self.a.squared_distance(p);
        }
        // Projection parameter t in [0, 1] as a fraction num/len2, kept in integers.
        let num = ((px - ax) * dx + (py - ay) * dy).clamp(0, len2);
        // Closest point = a + t * d, computed with rounding to the nearest nanometre.
        let cx = ax + div_round(num * dx, len2);
        let cy = ay + div_round(num * dy, len2);
        let ex = px - cx;
        let ey = py - cy;
        ex * ex + ey * ey
    }

    /// Squared distance between two segments: zero when they cross or touch.
    pub fn squared_distance_to_segment(&self, other: &Segment) -> i128 {
        if self.intersects(other) {
            return 0;
        }
        self.squared_distance_to_point(other.a)
            .min(self.squared_distance_to_point(other.b))
            .min(other.squared_distance_to_point(self.a))
            .min(other.squared_distance_to_point(self.b))
    }

    /// Proper or touching intersection, including collinear overlap.
    pub fn intersects(&self, other: &Segment) -> bool {
        let d1 = orient(other.a, other.b, self.a);
        let d2 = orient(other.a, other.b, self.b);
        let d3 = orient(self.a, self.b, other.a);
        let d4 = orient(self.a, self.b, other.b);
        if ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0)) {
            return true;
        }
        (d1 == 0 && other.bbox().contains(self.a))
            || (d2 == 0 && other.bbox().contains(self.b))
            || (d3 == 0 && self.bbox().contains(other.a))
            || (d4 == 0 && self.bbox().contains(other.b))
    }
}

fn div_round(n: i128, d: i128) -> i128 {
    // d > 0 always here.
    if n >= 0 { (n + d / 2) / d } else { -((-n + d / 2) / d) }
}

/// Sign of the turn a -> b -> c: positive counter-clockwise, zero collinear.
fn orient(a: Point, b: Point, c: Point) -> i128 {
    let abx = (b.x - a.x).0 as i128;
    let aby = (b.y - a.y).0 as i128;
    let acx = (c.x - a.x).0 as i128;
    let acy = (c.y - a.y).0 as i128;
    (abx * acy - aby * acx).signum()
}

/// A simple closed polygon. The last vertex connects back to the first; do not repeat it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Polygon {
    pub vertices: Vec<Point>,
}

impl Polygon {
    pub fn new(vertices: Vec<Point>) -> Polygon {
        Polygon { vertices }
    }

    pub fn rect(r: Rect) -> Polygon {
        Polygon { vertices: r.corners().to_vec() }
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.len() < 3
    }

    pub fn edges(&self) -> impl Iterator<Item = Segment> + '_ {
        let n = self.vertices.len();
        (0..n).map(move |i| Segment::new(self.vertices[i], self.vertices[(i + 1) % n]))
    }

    pub fn bbox(&self) -> Option<Rect> {
        let mut it = self.vertices.iter();
        let first = *it.next()?;
        Some(it.fold(Rect::from_points(first, first), |r, p| r.union(&Rect::from_points(*p, *p))))
    }

    /// Twice the signed area (shoelace), positive when counter-clockwise with y up.
    pub fn doubled_signed_area(&self) -> i128 {
        self.edges().map(|e| (e.a.x.0 as i128) * (e.b.y.0 as i128) - (e.b.x.0 as i128) * (e.a.y.0 as i128)).sum()
    }

    pub fn is_counter_clockwise(&self) -> bool {
        self.doubled_signed_area() > 0
    }

    /// Even-odd point-in-polygon. Points exactly on an edge count as inside.
    pub fn contains(&self, p: Point) -> bool {
        if self.is_empty() {
            return false;
        }
        for e in self.edges() {
            if orient(e.a, e.b, p) == 0 && e.bbox().contains(p) {
                return true;
            }
        }
        let mut inside = false;
        for e in self.edges() {
            let (a, b) = (e.a, e.b);
            if (a.y > p.y) != (b.y > p.y) {
                // x of the edge at height p.y, compared without division by cross-multiplying.
                let dy = (b.y - a.y).0 as i128;
                let lhs = ((p.x - a.x).0 as i128) * dy;
                let rhs = ((b.x - a.x).0 as i128) * ((p.y - a.y).0 as i128);
                if (dy > 0 && lhs < rhs) || (dy < 0 && lhs > rhs) {
                    inside = !inside;
                }
            }
        }
        inside
    }

    pub fn translate(&self, by: Point) -> Polygon {
        Polygon { vertices: self.vertices.iter().map(|v| v.translate(by)).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::from_points(Point::mm(3, 1), Point::mm(0, 4));
        assert_eq!(r.min, Point::mm(0, 1));
        assert_eq!(r.width(), Nm::mm(3));
        assert_eq!(r.height(), Nm::mm(3));
        assert!(r.contains(Point::mm(0, 1)));
        assert!(!r.contains(Point::mm(4, 1)));
        let c = Rect::centered(Point::mm(5, 5), Nm::mm(2), Nm::mm(4));
        assert_eq!(c.min, Point::mm(4, 3));
        assert_eq!(c.max, Point::mm(6, 7));
        assert_eq!(c.center(), Point::mm(5, 5));
    }

    #[test]
    fn rect_gap() {
        let a = Rect::from_points(Point::mm(0, 0), Point::mm(1, 1));
        let b = Rect::from_points(Point::mm(2, 0), Point::mm(3, 1));
        assert_eq!(a.squared_gap(&b), Nm::mm(1).squared());
        let c = Rect::from_points(Point::mm(2, 2), Point::mm(3, 3));
        assert_eq!(a.squared_gap(&c), 2 * Nm::mm(1).squared());
        assert_eq!(a.squared_gap(&a.inflate(Nm::mm(-0))), 0);
        assert!(a.intersects(&Rect::from_points(Point::mm(1, 1), Point::mm(2, 2))));
        assert!(!a.intersects(&b));
    }

    #[test]
    fn segment_point_distance() {
        let s = Segment::new(Point::mm(0, 0), Point::mm(10, 0));
        assert_eq!(s.squared_distance_to_point(Point::mm(5, 3)), Nm::mm(3).squared());
        assert_eq!(s.squared_distance_to_point(Point::mm(-4, 0)), Nm::mm(4).squared());
        assert_eq!(s.squared_distance_to_point(Point::mm(13, 4)), Nm::mm(5).squared());
        let dot = Segment::new(Point::mm(1, 1), Point::mm(1, 1));
        assert_eq!(dot.squared_distance_to_point(Point::mm(1, 3)), Nm::mm(2).squared());
    }

    #[test]
    fn segment_intersection() {
        let a = Segment::new(Point::mm(0, 0), Point::mm(4, 4));
        let b = Segment::new(Point::mm(0, 4), Point::mm(4, 0));
        assert!(a.intersects(&b));
        assert_eq!(a.squared_distance_to_segment(&b), 0);
        let c = Segment::new(Point::mm(5, 0), Point::mm(5, 4));
        assert!(!a.intersects(&c));
        assert_eq!(a.squared_distance_to_segment(&c), Nm::mm(1).squared());
        // Touching at an endpoint counts.
        let d = Segment::new(Point::mm(4, 4), Point::mm(8, 4));
        assert!(a.intersects(&d));
        // Collinear overlap counts.
        let e = Segment::new(Point::mm(2, 2), Point::mm(6, 6));
        assert!(a.intersects(&e));
        // Parallel, apart.
        let f = Segment::new(Point::mm(0, 1), Point::mm(4, 5));
        assert!(!a.intersects(&f));
    }

    #[test]
    fn polygon_area_and_containment() {
        let sq = Polygon::rect(Rect::from_points(Point::mm(0, 0), Point::mm(10, 10)));
        assert_eq!(sq.doubled_signed_area(), 2 * 100 * Nm::mm(1).squared());
        assert!(sq.is_counter_clockwise());
        assert!(sq.contains(Point::mm(5, 5)));
        assert!(sq.contains(Point::mm(0, 5)));
        assert!(sq.contains(Point::mm(10, 10)));
        assert!(!sq.contains(Point::mm(11, 5)));
        assert_eq!(sq.bbox(), Some(Rect::from_points(Point::mm(0, 0), Point::mm(10, 10))));

        // L shape: the notch is outside.
        let l = Polygon::new(vec![Point::mm(0, 0), Point::mm(10, 0), Point::mm(10, 5), Point::mm(5, 5), Point::mm(5, 10), Point::mm(0, 10)]);
        assert!(l.contains(Point::mm(2, 8)));
        assert!(!l.contains(Point::mm(8, 8)));
        assert_eq!(l.doubled_signed_area(), 2 * 75 * Nm::mm(1).squared());
    }

    #[test]
    fn rotation() {
        let p = Point::mm(2, 1);
        assert_eq!(p.rotate_quarter_turns(1), Point::mm(-1, 2));
        assert_eq!(p.rotate_quarter_turns(2), Point::mm(-2, -1));
        assert_eq!(p.rotate_quarter_turns(3), Point::mm(1, -2));
        assert_eq!(p.rotate_quarter_turns(4), p);
    }
}
