//! Item geometry (kurbo shapes in world coordinates) and the exact shape
//! tests used by hit testing.

use kurbo::{
    Affine, BezPath, Circle, Line, ParamCurve, ParamCurveArea, ParamCurveNearest, PathEl, PathSeg,
    Point, Rect, Shape,
};

/// Tolerance for converting circles into Bézier curves and for flattening
/// curves in rectangle tests (world units, i.e. 0.1 µm).
pub(crate) const CURVE_TOLERANCE: f64 = 1e-4;

/// The geometry of an item in world coordinates (millimetres, Y up).
///
/// Lines and circles are kept as primitives (cheap to store, exact hit
/// tests); everything else is a [`BezPath`].
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A line segment (only strokes are drawn; a zero-length line with round
    /// caps is a dot).
    Line(Line),
    /// A circle.
    Circle(Circle),
    /// Any path (lines, arcs as cubic Béziers, several subpaths).
    Path(BezPath),
}

impl Geometry {
    /// Returns the bounding box of the geometry itself (without stroke).
    pub fn bounding_box(&self) -> Rect {
        match self {
            Self::Line(l) => l.bounding_box(),
            Self::Circle(c) => c.bounding_box(),
            Self::Path(p) => {
                if p.elements().is_empty() {
                    Rect::ZERO
                } else {
                    p.bounding_box()
                }
            }
        }
    }

    /// Returns whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Line(_) | Self::Circle(_) => false,
            Self::Path(p) => p.elements().is_empty(),
        }
    }

    /// Returns the geometry mapped by an affine transformation. Circles stay
    /// circles under similarity transformations (rotation, mirroring,
    /// uniform scaling), otherwise they become paths.
    pub fn transformed(&self, affine: Affine) -> Self {
        match self {
            Self::Line(l) => Self::Line(affine * *l),
            Self::Circle(c) => {
                let [a, b, c2, d, _, _] = affine.as_coeffs();
                let is_similarity = ((a * a + b * b) - (c2 * c2 + d * d)).abs() < 1e-12
                    && (a * c2 + b * d).abs() < 1e-12;
                if is_similarity {
                    let scale = affine.determinant().abs().sqrt();
                    Self::Circle(Circle::new(affine * c.center, c.radius * scale))
                } else {
                    Self::Path(affine * c.to_path(CURVE_TOLERANCE))
                }
            }
            Self::Path(p) => Self::Path(affine * p.clone()),
        }
    }

    /// Converts the geometry into a path.
    pub fn to_path(&self) -> BezPath {
        let mut path = BezPath::new();
        self.append_to(&mut path, false);
        path
    }

    /// Appends the path elements to `path`. For fills, lines are skipped
    /// (they have no area).
    pub(crate) fn append_to(&self, path: &mut BezPath, fill: bool) {
        match self {
            Self::Line(l) => {
                if !fill {
                    path.move_to(l.p0);
                    // The stroker drops zero-length segments with their caps;
                    // a 1 nm segment keeps round caps visible as a dot.
                    let end = if l.p0 == l.p1 {
                        l.p1 + kurbo::Vec2::new(1e-6, 0.0)
                    } else {
                        l.p1
                    };
                    path.line_to(end);
                }
            }
            Self::Circle(c) => {
                let start = path.elements().len();
                path.extend(c.path_elements(CURVE_TOLERANCE));
                if fill && circle_winding_is_negative() {
                    let tail = BezPath::from_vec(path.elements()[start..].to_vec());
                    path.truncate(start);
                    path.extend(tail.reverse_subpaths().elements().iter().copied());
                }
            }
            Self::Path(p) => path.extend(p.elements().iter().copied()),
        }
    }

    /// Makes every subpath wind positively, so a non-zero fill of many
    /// merged items is their union (mirrored footprints flip the winding).
    pub(crate) fn normalize_winding(&mut self) {
        if let Self::Path(path) = self {
            normalize_winding(path);
        }
    }

    /// Returns whether a point is inside the filled area.
    pub(crate) fn contains(&self, p: Point) -> bool {
        match self {
            Self::Line(_) => false,
            Self::Circle(c) => (p - c.center).hypot2() <= c.radius * c.radius,
            Self::Path(path) => path.winding(p) != 0,
        }
    }

    /// Returns whether a point is within `distance` of the outline.
    pub(crate) fn outline_within(&self, p: Point, distance: f64) -> bool {
        let d2 = distance * distance;
        match self {
            Self::Line(l) => l.nearest(p, 1e-9).distance_sq <= d2,
            Self::Circle(c) => ((p - c.center).hypot() - c.radius).abs() <= distance,
            Self::Path(path) => path.segments().any(|s| {
                // Cheap bounding box rejection before the exact test.
                s.bounding_box().inflate(distance, distance).contains(p)
                    && s.nearest(p, 1e-9).distance_sq <= d2
            }),
        }
    }

    /// Returns whether the outline, widened by `margin` on each side, touches
    /// the rectangle. Curves are flattened; the widened outline is
    /// approximated with square ends (at most `margin` too generous).
    pub(crate) fn outline_touches_rect(&self, rect: Rect, margin: f64) -> bool {
        let r = rect.inflate(margin, margin);
        match self {
            Self::Line(l) => line_intersects_rect(l.p0, l.p1, r),
            Self::Circle(c) => {
                // Nearest and farthest distances from the center to the rect.
                let nearest = Point::new(c.center.x.clamp(rect.x0, rect.x1), {
                    c.center.y.clamp(rect.y0, rect.y1)
                });
                let near = (nearest - c.center).hypot();
                let far = [
                    Point::new(rect.x0, rect.y0),
                    Point::new(rect.x1, rect.y0),
                    Point::new(rect.x0, rect.y1),
                    Point::new(rect.x1, rect.y1),
                ]
                .iter()
                .map(|q| (*q - c.center).hypot())
                .fold(0.0, f64::max);
                near <= c.radius + margin && far >= c.radius - margin
            }
            Self::Path(path) => {
                let mut hit = false;
                let mut start = Point::ZERO;
                let mut last = Point::ZERO;
                kurbo::flatten(path.elements().iter().copied(), CURVE_TOLERANCE, |el| {
                    if hit {
                        return;
                    }
                    match el {
                        PathEl::MoveTo(p) => {
                            start = p;
                            last = p;
                            hit = r.contains(p);
                        }
                        PathEl::LineTo(p) => {
                            hit = line_intersects_rect(last, p, r);
                            last = p;
                        }
                        PathEl::ClosePath => {
                            hit = line_intersects_rect(last, start, r);
                            last = start;
                        }
                        // flatten() only emits the elements above.
                        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
                    }
                });
                hit
            }
        }
    }
}

impl From<Line> for Geometry {
    fn from(l: Line) -> Self {
        Self::Line(l)
    }
}

impl From<Circle> for Geometry {
    fn from(c: Circle) -> Self {
        Self::Circle(c)
    }
}

impl From<BezPath> for Geometry {
    fn from(p: BezPath) -> Self {
        Self::Path(p)
    }
}

impl From<Rect> for Geometry {
    fn from(r: Rect) -> Self {
        Self::Path(r.to_path(CURVE_TOLERANCE))
    }
}

/// Whether kurbo's circle path winds negatively (then filled circles are
/// reversed to match normalized paths).
fn circle_winding_is_negative() -> bool {
    static NEGATIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NEGATIVE.get_or_init(|| {
        let path = BezPath::from_iter(Circle::new(Point::ZERO, 1.0).path_elements(0.1));
        signed_area(path.elements()) < 0.0
    })
}

/// Signed area of one subpath, including the implicit closing segment.
fn signed_area(elements: &[PathEl]) -> f64 {
    let mut area = 0.0;
    let mut start = None;
    let mut last = None;
    for seg in kurbo::segments(elements.iter().copied()) {
        area += seg.signed_area();
        start.get_or_insert(seg.start());
        last = Some(seg.end());
    }
    if let (Some(s), Some(e)) = (start, last) {
        area += PathSeg::Line(Line::new(e, s)).signed_area();
    }
    area
}

fn normalize_winding(path: &mut BezPath) {
    let els = path.elements();
    let starts: Vec<usize> = els
        .iter()
        .enumerate()
        .filter(|(_, el)| matches!(el, PathEl::MoveTo(_)))
        .map(|(i, _)| i)
        .collect();
    let needs_fix = starts.iter().enumerate().any(|(k, &s)| {
        let end = starts.get(k + 1).copied().unwrap_or(els.len());
        signed_area(&els[s..end]) < 0.0
    });
    if !needs_fix {
        return;
    }
    let mut out = BezPath::new();
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(els.len());
        let sub = BezPath::from_vec(els[s..end].to_vec());
        if signed_area(sub.elements()) < 0.0 {
            out.extend(sub.reverse_subpaths().elements().iter().copied());
        } else {
            out.extend(sub.elements().iter().copied());
        }
    }
    *path = out;
}

/// Liang–Barsky test whether the segment `a`-`b` touches the rectangle.
fn line_intersects_rect(a: Point, b: Point, r: Rect) -> bool {
    let d = b - a;
    let mut t0: f64 = 0.0;
    let mut t1: f64 = 1.0;
    for (p, q) in [
        (-d.x, a.x - r.x0),
        (d.x, r.x1 - a.x),
        (-d.y, a.y - r.y0),
        (d.y, r.y1 - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winding_is_normalized() {
        let mut ccw = Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.1);
        let mut cw = ccw.reverse_subpaths();
        let area = signed_area(ccw.elements());
        assert!(area.abs() > 0.99);
        normalize_winding(&mut ccw);
        normalize_winding(&mut cw);
        assert!(signed_area(ccw.elements()) > 0.0);
        assert!(signed_area(cw.elements()) > 0.0);
    }

    #[test]
    fn filled_circle_winds_like_normalized_paths() {
        let mut p = BezPath::new();
        Geometry::Circle(Circle::new((0.0, 0.0), 1.0)).append_to(&mut p, true);
        assert!(signed_area(p.elements()) > 0.0);
    }

    #[test]
    fn rect_line_intersection() {
        let r = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(line_intersects_rect(
            Point::new(-1.0, 0.5),
            Point::new(2.0, 0.5),
            r
        ));
        assert!(line_intersects_rect(
            Point::new(0.5, 0.5),
            Point::new(0.6, 0.6),
            r
        ));
        assert!(!line_intersects_rect(
            Point::new(-1.0, 2.0),
            Point::new(2.0, 1.5),
            r
        ));
        assert!(!line_intersects_rect(
            Point::new(-1.0, -1.0),
            Point::new(-1.0, 5.0),
            r
        ));
    }

    #[test]
    fn similarity_keeps_circles() {
        let c = Geometry::Circle(Circle::new((1.0, 0.0), 2.0));
        let rot = c.transformed(Affine::rotate(1.0) * Affine::FLIP_X);
        assert!(matches!(rot, Geometry::Circle(k) if (k.radius - 2.0).abs() < 1e-12));
        let skew = c.transformed(Affine::scale_non_uniform(1.0, 2.0));
        assert!(matches!(skew, Geometry::Path(_)));
    }
}
