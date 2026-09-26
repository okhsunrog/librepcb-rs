//! Port of the `ClipperOffset` class of clipper.cpp.

use crate::polytree::{NodeId, PolyTree};
use crate::{
    ClipType, Clipper, EndType, IntPoint, JoinType, Path, Paths, PolyFillType, PolyType, Result,
    orientation, reverse_path, round,
};

const PI: f64 = std::f64::consts::PI;
const TWO_PI: f64 = PI * 2.0;
const DEF_ARC_TOLERANCE: f64 = 0.25;
const TOLERANCE: f64 = 1.0e-20;

#[derive(Debug, Clone, Copy, Default)]
struct DoublePoint {
    x: f64,
    y: f64,
}

impl DoublePoint {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

fn get_unit_normal(pt1: IntPoint, pt2: IntPoint) -> DoublePoint {
    if pt2.x == pt1.x && pt2.y == pt1.y {
        return DoublePoint::new(0.0, 0.0);
    }
    let mut dx = pt2.x.wrapping_sub(pt1.x) as f64;
    let mut dy = pt2.y.wrapping_sub(pt1.y) as f64;
    let f = 1.0 / (dx * dx + dy * dy).sqrt();
    dx *= f;
    dy *= f;
    DoublePoint::new(dy, -dx)
}

#[derive(Debug, Clone)]
struct OffsetNode {
    contour: Path,
    join_type: JoinType,
    end_type: EndType,
}

/// Offsets polygons and polylines (upstream `ClipperOffset`).
#[derive(Debug, Clone)]
pub struct ClipperOffset {
    /// Maximum distance (in multiples of delta) of mitered vertices from
    /// their original position.
    pub miter_limit: f64,
    /// Maximum distance of approximated arcs from the true arc.
    pub arc_tolerance: f64,
    dest_polys: Paths,
    src_poly: Path,
    dest_poly: Path,
    normals: Vec<DoublePoint>,
    delta: f64,
    sin_a: f64,
    sin: f64,
    cos: f64,
    miter_lim: f64,
    steps_per_rad: f64,
    /// Index of the path and vertex of the lowest vertex of all closed
    /// polygons (upstream `m_lowest`, with `X < 0` meaning none).
    lowest: Option<(usize, usize)>,
    /// Upstream `m_polyNodes`.
    nodes: Vec<OffsetNode>,
}

impl Default for ClipperOffset {
    fn default() -> Self {
        Self::new(2.0, DEF_ARC_TOLERANCE)
    }
}

impl ClipperOffset {
    /// Creates an offsetter with the given miter limit and arc tolerance
    /// (upstream defaults: 2.0 and 0.25).
    pub fn new(miter_limit: f64, arc_tolerance: f64) -> Self {
        Self {
            miter_limit,
            arc_tolerance,
            dest_polys: Paths::new(),
            src_poly: Path::new(),
            dest_poly: Path::new(),
            normals: Vec::new(),
            delta: 0.0,
            sin_a: 0.0,
            sin: 0.0,
            cos: 0.0,
            miter_lim: 0.0,
            steps_per_rad: 0.0,
            lowest: None,
            nodes: Vec::new(),
        }
    }

    /// Returns the miter limit.
    pub fn miter_limit(&self) -> f64 {
        self.miter_limit
    }

    /// Sets the miter limit.
    pub fn set_miter_limit(&mut self, value: f64) {
        self.miter_limit = value;
    }

    /// Returns the arc tolerance.
    pub fn arc_tolerance(&self) -> f64 {
        self.arc_tolerance
    }

    /// Sets the arc tolerance.
    pub fn set_arc_tolerance(&mut self, value: f64) {
        self.arc_tolerance = value;
    }

    /// Removes all paths.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.lowest = None;
    }

    /// Adds a path to offset.
    pub fn add_path(&mut self, path: &[IntPoint], join_type: JoinType, end_type: EndType) {
        let Some(&first) = path.first() else {
            return;
        };
        let mut high_i = path.len() - 1;

        // Strip duplicate points from path and also get index to the lowest
        // point.
        if end_type == EndType::ClosedLine || end_type == EndType::ClosedPolygon {
            while high_i > 0 && path[0] == path[high_i] {
                high_i -= 1;
            }
        }
        let mut contour = Path::with_capacity(high_i + 1);
        contour.push(first);
        let mut j = 0;
        let mut k = 0;
        for &pt in &path[1..=high_i] {
            if contour[j] != pt {
                j += 1;
                contour.push(pt);
                if pt.y > contour[k].y || (pt.y == contour[k].y && pt.x < contour[k].x) {
                    k = j;
                }
            }
        }
        if end_type == EndType::ClosedPolygon && j < 2 {
            return;
        }
        let lowest_pt = contour[k];
        self.nodes.push(OffsetNode {
            contour,
            join_type,
            end_type,
        });

        // If this path's lowest pt is lower than all the others then update
        // m_lowest.
        if end_type != EndType::ClosedPolygon {
            return;
        }
        let index = self.nodes.len() - 1;
        match self.lowest {
            None => self.lowest = Some((index, k)),
            Some((li, lk)) => {
                let ip = self.nodes[li].contour[lk];
                if lowest_pt.y > ip.y || (lowest_pt.y == ip.y && lowest_pt.x < ip.x) {
                    self.lowest = Some((index, k));
                }
            }
        }
    }

    /// Adds multiple paths to offset.
    pub fn add_paths(&mut self, paths: &[Path], join_type: JoinType, end_type: EndType) {
        for path in paths {
            self.add_path(path, join_type, end_type);
        }
    }

    fn fix_orientations(&mut self) {
        // Fixup orientations of all closed paths if the orientation of the
        // closed path with the lowermost vertex is wrong.
        let lowest_wrong = self
            .lowest
            .is_some_and(|(li, _)| !orientation(&self.nodes[li].contour));
        for node in &mut self.nodes {
            let reverse = if lowest_wrong {
                node.end_type == EndType::ClosedPolygon
                    || (node.end_type == EndType::ClosedLine && orientation(&node.contour))
            } else {
                node.end_type == EndType::ClosedLine && !orientation(&node.contour)
            };
            if reverse {
                reverse_path(&mut node.contour);
            }
        }
    }

    /// Offsets all paths by `delta` and returns the resulting polygons.
    pub fn execute(&mut self, delta: f64) -> Result<Paths> {
        self.fix_orientations();
        self.do_offset(delta);

        // Now clean up 'corners'.
        let mut clpr = Clipper::new();
        clpr.add_paths(&self.dest_polys, PolyType::Subject, true)?;
        if delta > 0.0 {
            clpr.execute(
                ClipType::Union,
                PolyFillType::Positive,
                PolyFillType::Positive,
            )
        } else {
            let outer = Self::outer_rect(&clpr);
            clpr.add_path(&outer, PolyType::Subject, true)?;
            clpr.set_reverse_solution(true);
            let mut solution = clpr.execute(
                ClipType::Union,
                PolyFillType::Negative,
                PolyFillType::Negative,
            )?;
            if !solution.is_empty() {
                solution.remove(0);
            }
            Ok(solution)
        }
    }

    /// Offsets all paths by `delta` and returns the result as tree.
    pub fn execute_tree(&mut self, delta: f64) -> Result<PolyTree> {
        self.fix_orientations();
        self.do_offset(delta);

        // Now clean up 'corners'.
        let mut clpr = Clipper::new();
        clpr.add_paths(&self.dest_polys, PolyType::Subject, true)?;
        if delta > 0.0 {
            clpr.execute_tree(
                ClipType::Union,
                PolyFillType::Positive,
                PolyFillType::Positive,
            )
        } else {
            let outer = Self::outer_rect(&clpr);
            clpr.add_path(&outer, PolyType::Subject, true)?;
            clpr.set_reverse_solution(true);
            let mut solution = clpr.execute_tree(
                ClipType::Union,
                PolyFillType::Negative,
                PolyFillType::Negative,
            )?;
            // Remove the outer PolyNode rectangle.
            let outer_node = solution.root.children.first().copied();
            match outer_node {
                Some(outer_node)
                    if solution.root.children.len() == 1
                        && !solution.nodes[outer_node].children.is_empty() =>
                {
                    let outer_children = solution.nodes[outer_node].children.clone();
                    solution.root.children[0] = outer_children[0];
                    solution.nodes[outer_children[0]].parent = solution.nodes[outer_node].parent;
                    for &child in &outer_children[1..] {
                        solution.add_child(NodeId::Root, child);
                    }
                }
                _ => solution.clear(),
            }
            Ok(solution)
        }
    }

    fn outer_rect(clpr: &Clipper) -> Path {
        let r = clpr.bounds();
        vec![
            IntPoint::new(r.left.wrapping_sub(10), r.bottom.wrapping_add(10)),
            IntPoint::new(r.right.wrapping_add(10), r.bottom.wrapping_add(10)),
            IntPoint::new(r.right.wrapping_add(10), r.top.wrapping_sub(10)),
            IntPoint::new(r.left.wrapping_sub(10), r.top.wrapping_sub(10)),
        ]
    }

    fn offset_pt(&self, pt: IntPoint, x: f64, y: f64) -> IntPoint {
        IntPoint::new(
            round(pt.x as f64 + x * self.delta),
            round(pt.y as f64 + y * self.delta),
        )
    }

    fn do_offset(&mut self, delta: f64) {
        self.dest_polys.clear();
        self.delta = delta;

        // If Zero offset, just copy any CLOSED polygons to m_p and return.
        if delta > -TOLERANCE && delta < TOLERANCE {
            self.dest_polys.reserve(self.nodes.len());
            for node in &self.nodes {
                if node.end_type == EndType::ClosedPolygon {
                    self.dest_polys.push(node.contour.clone());
                }
            }
            return;
        }

        // See offset_triginometry3.svg in the documentation folder.
        if self.miter_limit > 2.0 {
            self.miter_lim = 2.0 / (self.miter_limit * self.miter_limit);
        } else {
            self.miter_lim = 0.5;
        }

        let y = if self.arc_tolerance <= 0.0 {
            DEF_ARC_TOLERANCE
        } else if self.arc_tolerance > delta.abs() * DEF_ARC_TOLERANCE {
            delta.abs() * DEF_ARC_TOLERANCE
        } else {
            self.arc_tolerance
        };
        // See offset_triginometry2.svg in the documentation folder.
        let mut steps = PI / (1.0 - y / delta.abs()).acos();
        if steps > delta.abs() * PI {
            steps = delta.abs() * PI; // Ie excessive precision check.
        }
        self.sin = (TWO_PI / steps).sin();
        self.cos = (TWO_PI / steps).cos();
        self.steps_per_rad = steps / TWO_PI;
        if delta < 0.0 {
            self.sin = -self.sin;
        }

        self.dest_polys.reserve(self.nodes.len() * 2);
        for node_index in 0..self.nodes.len() {
            let node = &self.nodes[node_index];
            let (join_type, end_type) = (node.join_type, node.end_type);
            self.src_poly.clone_from(&node.contour);

            let len = self.src_poly.len();
            if len == 0 || (delta <= 0.0 && (len < 3 || end_type != EndType::ClosedPolygon)) {
                continue;
            }

            self.dest_poly.clear();
            if len == 1 {
                let src = self.src_poly[0];
                if join_type == JoinType::Round {
                    let (mut x, mut y) = (1.0, 0.0);
                    let mut j: i64 = 1;
                    while (j as f64) <= steps {
                        self.dest_poly.push(self.offset_pt(src, x, y));
                        let x2 = x;
                        x = x * self.cos - self.sin * y;
                        y = x2 * self.sin + y * self.cos;
                        j += 1;
                    }
                } else {
                    let (mut x, mut y) = (-1.0, -1.0);
                    for _ in 0..4 {
                        self.dest_poly.push(self.offset_pt(src, x, y));
                        if x < 0.0 {
                            x = 1.0;
                        } else if y < 0.0 {
                            y = 1.0;
                        } else {
                            x = -1.0;
                        }
                    }
                }
                self.dest_polys.push(self.dest_poly.clone());
                continue;
            }

            // Build m_normals.
            self.normals.clear();
            self.normals.reserve(len);
            for j in 0..len - 1 {
                self.normals
                    .push(get_unit_normal(self.src_poly[j], self.src_poly[j + 1]));
            }
            if end_type == EndType::ClosedLine || end_type == EndType::ClosedPolygon {
                self.normals
                    .push(get_unit_normal(self.src_poly[len - 1], self.src_poly[0]));
            } else {
                self.normals.push(self.normals[len - 2]);
            }

            if end_type == EndType::ClosedPolygon {
                let mut k = len - 1;
                for j in 0..len {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
            } else if end_type == EndType::ClosedLine {
                let mut k = len - 1;
                for j in 0..len {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
                self.dest_poly.clear();
                // Re-build m_normals.
                let n = self.normals[len - 1];
                for j in (1..len).rev() {
                    self.normals[j] =
                        DoublePoint::new(-self.normals[j - 1].x, -self.normals[j - 1].y);
                }
                self.normals[0] = DoublePoint::new(-n.x, -n.y);
                k = 0;
                for j in (0..len).rev() {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
            } else {
                let mut k = 0;
                for j in 1..len - 1 {
                    self.offset_point(j, &mut k, join_type);
                }

                if end_type == EndType::OpenButt {
                    let j = len - 1;
                    let (src, n) = (self.src_poly[j], self.normals[j]);
                    self.dest_poly.push(self.offset_pt(src, n.x, n.y));
                    self.dest_poly.push(IntPoint::new(
                        round(src.x as f64 - n.x * delta),
                        round(src.y as f64 - n.y * delta),
                    ));
                } else {
                    let j = len - 1;
                    k = len - 2;
                    self.sin_a = 0.0;
                    self.normals[j] = DoublePoint::new(-self.normals[j].x, -self.normals[j].y);
                    if end_type == EndType::OpenSquare {
                        self.do_square(j, k);
                    } else {
                        self.do_round(j, k);
                    }
                }

                // Re-build m_normals.
                for j in (1..len).rev() {
                    self.normals[j] =
                        DoublePoint::new(-self.normals[j - 1].x, -self.normals[j - 1].y);
                }
                self.normals[0] = DoublePoint::new(-self.normals[1].x, -self.normals[1].y);

                k = len - 1;
                for j in (1..k).rev() {
                    self.offset_point(j, &mut k, join_type);
                }

                if end_type == EndType::OpenButt {
                    let (src, n) = (self.src_poly[0], self.normals[0]);
                    self.dest_poly.push(IntPoint::new(
                        round(src.x as f64 - n.x * delta),
                        round(src.y as f64 - n.y * delta),
                    ));
                    self.dest_poly.push(self.offset_pt(src, n.x, n.y));
                } else {
                    self.sin_a = 0.0;
                    if end_type == EndType::OpenSquare {
                        self.do_square(0, 1);
                    } else {
                        self.do_round(0, 1);
                    }
                }
                self.dest_polys.push(self.dest_poly.clone());
            }
        }
    }

    fn offset_point(&mut self, j: usize, k: &mut usize, join_type: JoinType) {
        let (nj, nk) = (self.normals[j], self.normals[*k]);
        // Cross product.
        self.sin_a = nk.x * nj.y - nj.x * nk.y;
        if (self.sin_a * self.delta).abs() < 1.0 {
            // Dot product.
            let cos_a = nk.x * nj.x + nj.y * nk.y;
            if cos_a > 0.0 {
                // Angle => 0 degrees.
                let src = self.src_poly[j];
                self.dest_poly.push(self.offset_pt(src, nk.x, nk.y));
                return;
            }
            // Else angle => 180 degrees.
        } else {
            self.sin_a = self.sin_a.clamp(-1.0, 1.0);
        }

        if self.sin_a * self.delta < 0.0 {
            let src = self.src_poly[j];
            self.dest_poly.push(self.offset_pt(src, nk.x, nk.y));
            self.dest_poly.push(src);
            self.dest_poly.push(self.offset_pt(src, nj.x, nj.y));
        } else {
            match join_type {
                JoinType::Miter => {
                    let r = 1.0 + (nj.x * nk.x + nj.y * nk.y);
                    if r >= self.miter_lim {
                        self.do_miter(j, *k, r);
                    } else {
                        self.do_square(j, *k);
                    }
                }
                JoinType::Square => self.do_square(j, *k),
                JoinType::Round => self.do_round(j, *k),
            }
        }
        *k = j;
    }

    fn do_square(&mut self, j: usize, k: usize) {
        let (nj, nk) = (self.normals[j], self.normals[k]);
        let src = self.src_poly[j];
        let dx = (self.sin_a.atan2(nk.x * nj.x + nk.y * nj.y) / 4.0).tan();
        self.dest_poly.push(IntPoint::new(
            round(src.x as f64 + self.delta * (nk.x - nk.y * dx)),
            round(src.y as f64 + self.delta * (nk.y + nk.x * dx)),
        ));
        self.dest_poly.push(IntPoint::new(
            round(src.x as f64 + self.delta * (nj.x + nj.y * dx)),
            round(src.y as f64 + self.delta * (nj.y - nj.x * dx)),
        ));
    }

    fn do_miter(&mut self, j: usize, k: usize, r: f64) {
        let (nj, nk) = (self.normals[j], self.normals[k]);
        let src = self.src_poly[j];
        let q = self.delta / r;
        self.dest_poly.push(IntPoint::new(
            round(src.x as f64 + (nk.x + nj.x) * q),
            round(src.y as f64 + (nk.y + nj.y) * q),
        ));
    }

    fn do_round(&mut self, j: usize, k: usize) {
        let (nj, nk) = (self.normals[j], self.normals[k]);
        let src = self.src_poly[j];
        let a = self.sin_a.atan2(nk.x * nj.x + nk.y * nj.y);
        // Note: `(int)Round(...)` truncates to 32 bits like upstream.
        let steps = (round(self.steps_per_rad * a.abs()) as i32).max(1);

        let (mut x, mut y) = (nk.x, nk.y);
        for _ in 0..steps {
            self.dest_poly.push(self.offset_pt(src, x, y));
            let x2 = x;
            x = x * self.cos - self.sin * y;
            y = x2 * self.sin + y * self.cos;
        }
        self.dest_poly.push(self.offset_pt(src, nj.x, nj.y));
    }
}
