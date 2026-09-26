//! Emulation of the `QPainterPath` bounding rectangle of [`Path`]s (upstream
//! `Path::toQPainterPathPx(paths, false).boundingRect()`).
//!
//! The stroke font uses this bounding rectangle to align glyphs, so it
//! defines the geometry of stroke texts (and thus of exports). Qt converts
//! arcs into cubic Bézier curves (which deviate slightly from true arcs) and
//! computes the exact bounds of these curves. To get identical results, the
//! relevant parts of Qt 6 (`QPainterPath::arcTo()`, `qt_curves_for_arc()`,
//! `qt_t_for_arc_angle()`, `qt_find_ellipse_coords()`, `QBezier` and
//! `QPainterPath::computeBoundingRect()`, LGPL-3.0/GPL) are ported here;
//! no crate reproduces Qt's arc approximation. Qt's fuzzy point comparisons,
//! which drop degenerate elements, are reproduced as well.

use crate::geometry::Path;
use crate::utils::toolbox;

/// `QT_PATH_KAPPA`
const KAPPA: f64 = 0.5522847498;

/// An axis-aligned rectangle in graphics scene pixels (Y axis pointing
/// down), like `QRectF`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RectF {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl RectF {
    /// Returns the left edge.
    pub fn left(&self) -> f64 {
        self.x
    }

    /// Returns the right edge.
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Returns the top edge.
    pub fn top(&self) -> f64 {
        self.y
    }

    /// Returns the bottom edge.
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// Returns the bounding rectangle (in pixels) of the given paths as strokes
/// (not united), or an empty rectangle at the origin if there is nothing to
/// draw.
pub fn bounding_rect_px(paths: &[Path]) -> RectF {
    let mut elements: Vec<Element> = Vec::new();
    for path in paths {
        let sub = PainterPath::from_path(path);
        // QPainterPath::addPath(): empty paths (only a move-to) are skipped.
        if sub.elements.len() > 1 {
            elements.extend(sub.elements);
        }
    }
    compute_bounding_rect(&elements)
}

/// Returns whether a painter path built from `paths` (as in
/// [`bounding_rect_px()`]) would be empty (`QPainterPath::isEmpty()`), i.e.
/// no path draws anything.
pub fn is_empty_px(paths: &[Path]) -> bool {
    paths
        .iter()
        .all(|path| PainterPath::from_path(path).elements.len() <= 1)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ElementType {
    MoveTo,
    LineTo,
    CurveTo,
    CurveToData,
}

#[derive(Debug, Clone, Copy)]
struct Element {
    x: f64,
    y: f64,
    kind: ElementType,
}

impl Element {
    fn point(&self) -> (f64, f64) {
        (self.x, self.y)
    }
}

/// Minimal `QPainterPath` supporting what `Path::toQPainterPathPx()` does.
struct PainterPath {
    elements: Vec<Element>,
}

impl PainterPath {
    fn from_path(path: &Path) -> Self {
        let mut pp = Self {
            elements: vec![Element {
                x: 0.0,
                y: 0.0,
                kind: ElementType::MoveTo,
            }],
        };
        let vertices = path.vertices();
        for (i, v) in vertices.iter().enumerate() {
            if i == 0 {
                pp.move_to(v.pos.to_px());
                continue;
            }
            let v0 = vertices[i - 1];
            if let Some(center) = toolbox::arc_center(v0.pos, v.pos, v0.angle) {
                // Arc segment.
                let (cx, cy) = center.to_px();
                let (x0, y0) = v0.pos.to_px();
                let (dx, dy) = (x0 - cx, y0 - cy);
                let radius = (dx * dx + dy * dy).sqrt();
                let start_angle_deg = -(libm::atan2(dy, dx) * (180.0 / std::f64::consts::PI));
                pp.arc_to(
                    RectF {
                        x: cx - radius,
                        y: cy - radius,
                        width: radius * 2.0,
                        height: radius * 2.0,
                    },
                    start_angle_deg,
                    v0.angle.to_deg(),
                );
            } else {
                // Straight segment.
                pp.line_to(v.pos.to_px());
            }
        }
        pp
    }

    fn last(&self) -> Element {
        // There is always at least one element.
        self.elements[self.elements.len() - 1]
    }

    fn move_to(&mut self, (x, y): (f64, f64)) {
        if !is_valid_point((x, y)) {
            return;
        }
        let last = self.elements.len() - 1;
        if self.elements[last].kind == ElementType::MoveTo {
            self.elements[last].x = x;
            self.elements[last].y = y;
        } else {
            self.elements.push(Element {
                x,
                y,
                kind: ElementType::MoveTo,
            });
        }
    }

    fn line_to(&mut self, p: (f64, f64)) {
        if !is_valid_point(p) || fuzzy_point_eq(p, self.last().point()) {
            return;
        }
        self.elements.push(Element {
            x: p.0,
            y: p.1,
            kind: ElementType::LineTo,
        });
    }

    fn cubic_to(&mut self, c1: (f64, f64), c2: (f64, f64), e: (f64, f64)) {
        if !is_valid_point(c1) || !is_valid_point(c2) || !is_valid_point(e) {
            return;
        }
        // Abort on empty curve.
        if fuzzy_point_eq(self.last().point(), c1)
            && fuzzy_point_eq(c1, c2)
            && fuzzy_point_eq(c2, e)
        {
            return;
        }
        for ((x, y), kind) in [
            (c1, ElementType::CurveTo),
            (c2, ElementType::CurveToData),
            (e, ElementType::CurveToData),
        ] {
            self.elements.push(Element { x, y, kind });
        }
    }

    fn arc_to(&mut self, rect: RectF, start_angle: f64, sweep_length: f64) {
        if ![
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            start_angle,
            sweep_length,
        ]
        .iter()
        .all(|v| is_valid_coord(*v))
        {
            return;
        }
        if rect.width == 0.0 && rect.height == 0.0 {
            return; // QRectF::isNull()
        }
        let (curve_start, curves) = curves_for_arc(rect, start_angle, sweep_length);
        self.line_to(curve_start);
        for [c1, c2, e] in curves.as_chunks::<3>().0 {
            self.cubic_to(*c1, *c2, *e);
        }
    }
}

/// `qt_is_finite()` and the coordinate limit of `QPainterPath`.
fn is_valid_coord(c: f64) -> bool {
    c.is_finite() && c.abs() < 1e128
}

fn is_valid_point((x, y): (f64, f64)) -> bool {
    is_valid_coord(x) && is_valid_coord(y)
}

/// `qFuzzyIsNull(double)`
fn fuzzy_is_null(d: f64) -> bool {
    d.abs() <= 0.000000000001
}

/// `qFuzzyCompare(double, double)`
fn fuzzy_compare(p1: f64, p2: f64) -> bool {
    (p1 - p2).abs() * 1000000000000.0 <= p1.abs().min(p2.abs())
}

/// `QPointF::operator==()` (Qt 6)
fn fuzzy_point_eq(p1: (f64, f64), p2: (f64, f64)) -> bool {
    let eq = |a: f64, b: f64| {
        if a == 0.0 || b == 0.0 {
            fuzzy_is_null(a - b)
        } else {
            fuzzy_compare(a, b)
        }
    };
    eq(p1.0, p2.0) && eq(p1.1, p2.1)
}

/// `QBezier`
#[derive(Debug, Clone, Copy, Default)]
struct Bezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    x4: f64,
    y4: f64,
}

impl Bezier {
    fn from_points(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> Self {
        Self {
            x1: p1.0,
            y1: p1.1,
            x2: p2.0,
            y2: p2.1,
            x3: p3.0,
            y3: p3.1,
            x4: p4.0,
            y4: p4.1,
        }
    }

    fn coefficients(t: f64) -> (f64, f64, f64, f64) {
        let m_t = 1. - t;
        let mut b = m_t * m_t;
        let mut c = t * t;
        let d = c * t;
        let a = b * m_t;
        b *= 3. * t;
        c *= 3. * m_t;
        (a, b, c, d)
    }

    fn point_at(&self, t: f64) -> (f64, f64) {
        let m_t = 1. - t;
        let x = {
            let mut a = self.x1 * m_t + self.x2 * t;
            let mut b = self.x2 * m_t + self.x3 * t;
            let c = self.x3 * m_t + self.x4 * t;
            a = a * m_t + b * t;
            b = b * m_t + c * t;
            a * m_t + b * t
        };
        let y = {
            let mut a = self.y1 * m_t + self.y2 * t;
            let mut b = self.y2 * m_t + self.y3 * t;
            let c = self.y3 * m_t + self.y4 * t;
            a = a * m_t + b * t;
            b = b * m_t + c * t;
            a * m_t + b * t
        };
        (x, y)
    }

    /// `parameterSplitLeft()`: `self` becomes the right part.
    fn parameter_split_left(&mut self, t: f64) -> Self {
        let mut left = Self {
            x1: self.x1,
            y1: self.y1,
            ..Self::default()
        };
        left.x2 = self.x1 + t * (self.x2 - self.x1);
        left.y2 = self.y1 + t * (self.y2 - self.y1);
        left.x3 = self.x2 + t * (self.x3 - self.x2); // temporary holding spot
        left.y3 = self.y2 + t * (self.y3 - self.y2); // temporary holding spot
        self.x3 += t * (self.x4 - self.x3);
        self.y3 += t * (self.y4 - self.y3);
        self.x2 = left.x3 + t * (self.x3 - left.x3);
        self.y2 = left.y3 + t * (self.y3 - left.y3);
        left.x3 = left.x2 + t * (left.x3 - left.x2);
        left.y3 = left.y2 + t * (left.y3 - left.y2);
        left.x4 = left.x3 + t * (self.x2 - left.x3);
        self.x1 = left.x4;
        left.y4 = left.y3 + t * (self.y2 - left.y3);
        self.y1 = left.y4;
        left
    }

    fn bezier_on_interval(&self, t0: f64, t1: f64) -> Self {
        if t0 == 0.0 && t1 == 1.0 {
            return *self;
        }
        let mut bezier = *self;
        bezier.parameter_split_left(t0);
        let true_t = (t1 - t0) / (1.0 - t0);
        bezier.parameter_split_left(true_t)
    }

    /// `qt_painterpath_bezier_extrema()`
    fn extrema(&self) -> RectF {
        let (mut minx, mut maxx) = if self.x1 < self.x4 {
            (self.x1, self.x4)
        } else {
            (self.x4, self.x1)
        };
        let (mut miny, mut maxy) = if self.y1 < self.y4 {
            (self.y1, self.y4)
        } else {
            (self.y4, self.y1)
        };
        let mut check_t = |t: f64| {
            if (0.0..=1.0).contains(&t) {
                let (px, py) = self.point_at(t);
                if px < minx {
                    minx = px;
                } else if px > maxx {
                    maxx = px;
                }
                if py < miny {
                    miny = py;
                } else if py > maxy {
                    maxy = py;
                }
            }
        };

        // Update for the X extrema.
        let (p1, p2, p3, p4) = (self.x1, self.x2, self.x3, self.x4);
        let ax = 3. * (-p1 + 3. * p2 - 3. * p3 + p4);
        let bx = 6. * (p1 - 2. * p2 + p3);
        let cx = 3. * (-p1 + p2);
        if fuzzy_is_null(ax) {
            // Linear curves are covered by initialization.
            if !fuzzy_is_null(bx) {
                check_t(-cx / bx);
            }
        } else {
            let tx = bx * bx - 4. * ax * cx;
            if tx >= 0. {
                let temp = tx.sqrt();
                let rcp = 1. / (2. * ax);
                check_t((-bx + temp) * rcp);
                check_t((-bx - temp) * rcp);
            }
        }

        // Update for the Y extrema.
        let (p1, p2, p3, p4) = (self.y1, self.y2, self.y3, self.y4);
        let ay = 3. * (-p1 + 3. * p2 - 3. * p3 + p4);
        let by = 6. * (p1 - 2. * p2 + p3);
        let cy = 3. * (-p1 + p2);
        if fuzzy_is_null(ay) {
            if !fuzzy_is_null(by) {
                check_t(-cy / by);
            }
        } else {
            let ty = by * by - 4. * ay * cy;
            if ty > 0. {
                let temp = ty.sqrt();
                let rcp = 1. / (2. * ay);
                check_t((-by + temp) * rcp);
                check_t((-by - temp) * rcp);
            }
        }
        RectF {
            x: minx,
            y: miny,
            width: maxx - minx,
            height: maxy - miny,
        }
    }
}

/// `qt_t_for_arc_angle()`
fn t_for_arc_angle(angle: f64) -> f64 {
    if fuzzy_is_null(angle) {
        return 0.0;
    }
    if fuzzy_compare(angle, 90.0) {
        return 1.0;
    }
    let radians = angle * (std::f64::consts::PI / 180.0);
    let cos_angle = libm::cos(radians);
    let sin_angle = libm::sin(radians);
    let k = KAPPA;

    // Initial guess, then some iterations of Newton's method to approximate
    // cos_angle.
    let mut tc = angle / 90.0;
    for _ in 0..2 {
        tc -= ((((2. - 3. * k) * tc + 3. * (k - 1.)) * tc) * tc + 1. - cos_angle)
            / (((6. - 9. * k) * tc + 6. * (k - 1.)) * tc);
    }

    // Same for sin_angle.
    let mut ts = tc;
    for _ in 0..2 {
        ts -= ((((3. * k - 2.) * ts - 6. * k + 3.) * ts + 3. * k) * ts - sin_angle)
            / (((9. * k - 6.) * ts + 12. * k - 6.) * ts + 3. * k);
    }

    // Use the average of both.
    0.5 * (tc + ts)
}

/// `qt_find_ellipse_coords()` (start and end point)
fn find_ellipse_coords(r: RectF, angle: f64, length: f64) -> [(f64, f64); 2] {
    if r.width == 0.0 && r.height == 0.0 {
        return [(0.0, 0.0); 2];
    }
    let w2 = r.width / 2.0;
    let h2 = r.height / 2.0;
    let center = (r.x + r.width / 2.0, r.y + r.height / 2.0);
    [angle, angle + length].map(|a| {
        let theta = a - 360.0 * (a / 360.0).floor();
        let mut t = theta / 90.0;
        // Truncate.
        let quadrant = t as i32;
        t -= f64::from(quadrant);
        t = t_for_arc_angle(90.0 * t);
        // Swap x and y?
        if quadrant & 1 != 0 {
            t = 1.0 - t;
        }
        let (a, b, c, d) = Bezier::coefficients(t);
        let mut p = (a + b + c * KAPPA, d + c + b * KAPPA);
        // Left quadrants.
        if quadrant == 1 || quadrant == 2 {
            p.0 = -p.0;
        }
        // Top quadrants.
        if quadrant == 0 || quadrant == 1 {
            p.1 = -p.1;
        }
        (center.0 + w2 * p.0, center.1 + h2 * p.1)
    })
}

/// `qt_curves_for_arc()`: returns the start point and the curve points (3
/// per cubic curve).
fn curves_for_arc(
    rect: RectF,
    start_angle: f64,
    sweep_length: f64,
) -> ((f64, f64), Vec<(f64, f64)>) {
    let mut curves = Vec::new();
    if [
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        start_angle,
        sweep_length,
    ]
    .iter()
    .any(|v| v.is_nan())
    {
        return ((0.0, 0.0), curves);
    }
    if rect.width == 0.0 && rect.height == 0.0 {
        return ((0.0, 0.0), curves);
    }

    let (x, y) = (rect.x, rect.y);
    let w = rect.width;
    let w2 = rect.width / 2.0;
    let w2k = w2 * KAPPA;
    let h = rect.height;
    let h2 = rect.height / 2.0;
    let h2k = h2 * KAPPA;

    let points: [(f64, f64); 13] = [
        // start point
        (x + w, y + h2),
        // 0 -> 270 degrees
        (x + w, y + h2 + h2k),
        (x + w2 + w2k, y + h),
        (x + w2, y + h),
        // 270 -> 180 degrees
        (x + w2 - w2k, y + h),
        (x, y + h2 + h2k),
        (x, y + h2),
        // 180 -> 90 degrees
        (x, y + h2 - h2k),
        (x + w2 - w2k, y),
        (x + w2, y),
        // 90 -> 0 degrees
        (x + w2 + w2k, y),
        (x + w, y + h2 - h2k),
        (x + w, y + h2),
    ];

    let sweep_length = sweep_length.clamp(-360.0, 360.0);

    // Special case fast paths.
    if start_angle == 0.0 {
        if sweep_length == 360.0 {
            curves.extend(points[..12].iter().rev());
            return (points[12], curves);
        } else if sweep_length == -360.0 {
            curves.extend(&points[1..=12]);
            return (points[0], curves);
        }
    }

    // `int(qFloor(...))`: saturating instead of undefined behavior.
    let mut start_segment = (start_angle / 90.0).floor() as i32;
    let mut end_segment = ((start_angle + sweep_length) / 90.0).floor() as i32;

    let mut start_t = (start_angle - f64::from(start_segment) * 90.0) / 90.0;
    let mut end_t = (start_angle + sweep_length - f64::from(end_segment) * 90.0) / 90.0;

    let delta = if sweep_length > 0.0 { 1 } else { -1 };
    if delta < 0 {
        start_t = 1.0 - start_t;
        end_t = 1.0 - end_t;
    }

    // Avoid empty start segment.
    if fuzzy_is_null(start_t - 1.0) {
        start_t = 0.0;
        start_segment += delta;
    }

    // Avoid empty end segment.
    if fuzzy_is_null(end_t) {
        end_t = 1.0;
        end_segment -= delta;
    }

    start_t = t_for_arc_angle(start_t * 90.0);
    end_t = t_for_arc_angle(end_t * 90.0);

    let split_at_start = !fuzzy_is_null(start_t);
    let split_at_end = !fuzzy_is_null(end_t - 1.0);

    let end = end_segment + delta;

    let quadrant_index = |i: i32| 3 * (3 - i.rem_euclid(4)) as usize;

    // Empty arc?
    if start_segment == end {
        let j = quadrant_index(start_segment);
        return (if delta > 0 { points[j + 3] } else { points[j] }, curves);
    }

    let [start_point, end_point] = find_ellipse_coords(rect, start_angle, sweep_length);

    let mut i = start_segment;
    while i != end {
        let j = quadrant_index(i);
        let mut b = if delta > 0 {
            Bezier::from_points(points[j + 3], points[j + 2], points[j + 1], points[j])
        } else {
            Bezier::from_points(points[j], points[j + 1], points[j + 2], points[j + 3])
        };

        // Empty arc?
        if start_segment == end_segment && fuzzy_compare(start_t, end_t) {
            return (start_point, curves);
        }

        if i == start_segment {
            if i == end_segment && split_at_end {
                b = b.bezier_on_interval(start_t, end_t);
            } else if split_at_start {
                b = b.bezier_on_interval(start_t, 1.0);
            }
        } else if i == end_segment && split_at_end {
            b = b.bezier_on_interval(0.0, end_t);
        }

        // Push control points.
        curves.push((b.x2, b.y2));
        curves.push((b.x3, b.y3));
        curves.push((b.x4, b.y4));
        i += delta;
    }

    if let Some(last) = curves.last_mut() {
        *last = end_point;
    }
    (start_point, curves)
}

/// `QPainterPath::computeBoundingRect()`
fn compute_bounding_rect(elements: &[Element]) -> RectF {
    let Some(first) = elements.first() else {
        return RectF::default();
    };
    let (mut minx, mut maxx) = (first.x, first.x);
    let (mut miny, mut maxy) = (first.y, first.y);
    let mut i = 1;
    while i < elements.len() {
        let e = elements[i];
        match e.kind {
            ElementType::MoveTo | ElementType::LineTo => {
                if e.x > maxx {
                    maxx = e.x;
                } else if e.x < minx {
                    minx = e.x;
                }
                if e.y > maxy {
                    maxy = e.y;
                } else if e.y < miny {
                    miny = e.y;
                }
            }
            ElementType::CurveTo => {
                let point = |k: usize| elements.get(k).map_or((0.0, 0.0), Element::point);
                let b = Bezier::from_points(point(i - 1), e.point(), point(i + 1), point(i + 2));
                let r = b.extrema();
                let right = r.right();
                let bottom = r.bottom();
                if r.x < minx {
                    minx = r.x;
                }
                if right > maxx {
                    maxx = right;
                }
                if r.y < miny {
                    miny = r.y;
                }
                if bottom > maxy {
                    maxy = bottom;
                }
                i += 2;
            }
            ElementType::CurveToData => {}
        }
        i += 1;
    }
    RectF {
        x: minx,
        y: miny,
        width: maxx - minx,
        height: maxy - miny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vertex;
    use crate::types::{Angle, Point};

    #[test]
    fn straight_lines() {
        let path = Path::new(vec![
            Vertex::at(Point::from_nm(0, 0)),
            Vertex::at(Point::from_nm(25_400_000, -25_400_000)),
        ]);
        let r = bounding_rect_px(&[path]);
        assert_eq!((r.left(), r.top()), (0.0, 0.0));
        assert!((r.right() - 72.0).abs() < 1e-9);
        assert!((r.bottom() - 72.0).abs() < 1e-9);
    }

    #[test]
    fn full_circle() {
        // Qt's Bézier approximation of arcs is very close to the true arc.
        let r = bounding_rect_px(&[Path::circle(
            crate::types::PositiveLength::new(crate::types::Length::new(25_400_000)).unwrap(),
        )]);
        assert!((r.width - 72.0).abs() < 1e-3, "{r:?}");
        assert!((r.height - 72.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn empty() {
        assert_eq!(bounding_rect_px(&[]), RectF::default());
        let single = Path::new(vec![Vertex::new(Point::from_nm(5, 5), Angle::DEG90)]);
        assert_eq!(bounding_rect_px(&[single]), RectF::default());
    }

    #[test]
    fn t_for_angle() {
        assert_eq!(t_for_arc_angle(0.0), 0.0);
        assert_eq!(t_for_arc_angle(90.0), 1.0);
        assert!((t_for_arc_angle(45.0) - 0.5).abs() < 1e-9);
    }
}
