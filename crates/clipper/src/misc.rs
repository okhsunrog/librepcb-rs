//! Port of the miscellaneous public functions of clipper.cpp.

use crate::poly_tree::{NodeId, PolyTree};
use crate::{ClipType, Clipper, IntPoint, Path, Paths, PolyFillType, PolyType, Result};

/// Returns the orientation of a polygon: `true` for a non-negative
/// [`area()`] (counter-clockwise with the Y axis pointing up).
pub fn orientation(poly: &[IntPoint]) -> bool {
    area(poly) >= 0.0
}

/// Returns the signed area of a polygon (positive if counter-clockwise with
/// the Y axis pointing up), 0 for less than 3 vertices.
pub fn area(poly: &[IntPoint]) -> f64 {
    let size = poly.len();
    if size < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut j = size - 1;
    for i in 0..size {
        a += (poly[j].x as f64 + poly[i].x as f64) * (poly[j].y as f64 - poly[i].y as f64);
        j = i;
    }
    -a * 0.5
}

/// Returns whether a point is inside a polygon: 0 if outside, +1 if inside,
/// -1 if on the boundary.
///
/// See "The Point in Polygon Problem for Arbitrary Polygons" by Hormann &
/// Agathos.
pub fn point_in_polygon(pt: IntPoint, path: &[IntPoint]) -> i32 {
    let mut result = 0;
    let cnt = path.len();
    if cnt < 3 {
        return 0;
    }
    let mut ip = path[0];
    for i in 1..=cnt {
        let ip_next = if i == cnt { path[0] } else { path[i] };
        if ip_next.y == pt.y
            && (ip_next.x == pt.x || (ip.y == pt.y && ((ip_next.x > pt.x) == (ip.x < pt.x))))
        {
            return -1;
        }
        if (ip.y < pt.y) != (ip_next.y < pt.y) {
            let cross = || {
                ip.x.wrapping_sub(pt.x) as f64 * ip_next.y.wrapping_sub(pt.y) as f64
                    - ip_next.x.wrapping_sub(pt.x) as f64 * ip.y.wrapping_sub(pt.y) as f64
            };
            if ip.x >= pt.x {
                if ip_next.x > pt.x {
                    result = 1 - result;
                } else {
                    let d = cross();
                    if d == 0.0 {
                        return -1;
                    }
                    if (d > 0.0) == (ip_next.y > ip.y) {
                        result = 1 - result;
                    }
                }
            } else if ip_next.x > pt.x {
                let d = cross();
                if d == 0.0 {
                    return -1;
                }
                if (d > 0.0) == (ip_next.y > ip.y) {
                    result = 1 - result;
                }
            }
        }
        ip = ip_next;
    }
    result
}

/// Reverses the vertex order of a path.
pub fn reverse_path(p: &mut Path) {
    p.reverse();
}

/// Reverses the vertex order of all paths.
pub fn reverse_paths(p: &mut Paths) {
    p.iter_mut().for_each(reverse_path);
}

/// Removes self-intersections from a polygon (union with strictly simple
/// output).
pub fn simplify_polygon(in_poly: &[IntPoint], fill_type: PolyFillType) -> Result<Paths> {
    let mut c = Clipper::new();
    c.set_strictly_simple(true);
    c.add_path(in_poly, PolyType::Subject, true)?;
    c.execute(ClipType::Union, fill_type, fill_type)
}

/// Removes self-intersections from polygons (union with strictly simple
/// output).
pub fn simplify_polygons(in_polys: &[Path], fill_type: PolyFillType) -> Result<Paths> {
    let mut c = Clipper::new();
    c.set_strictly_simple(true);
    c.add_paths(in_polys, PolyType::Subject, true)?;
    c.execute(ClipType::Union, fill_type, fill_type)
}

fn distance_from_line_sqrd(pt: IntPoint, ln1: IntPoint, ln2: IntPoint) -> f64 {
    // The equation of a line in general form (Ax + By + C = 0) given 2
    // points (x1,y1) & (x2,y2) is:
    // (y1 - y2)x + (x2 - x1)y + (y2 - y1)x1 - (x2 - x1)y1 = 0
    // A = (y1 - y2); B = (x2 - x1); C = (y2 - y1)x1 - (x2 - x1)y1
    // Perpendicular distance of point (x3,y3) = (Ax3 + By3 + C)/Sqrt(A^2 + B^2)
    let a = ln1.y.wrapping_sub(ln2.y) as f64;
    let b = ln2.x.wrapping_sub(ln1.x) as f64;
    let mut c = a * ln1.x as f64 + b * ln1.y as f64;
    c = a * pt.x as f64 + b * pt.y as f64 - c;
    (c * c) / (a * a + b * b)
}

fn slopes_near_collinear(pt1: IntPoint, pt2: IntPoint, pt3: IntPoint, dist_sqrd: f64) -> bool {
    // This function is more accurate when the point that's geometrically
    // between the other 2 points is the one that's tested for distance.
    // Ie makes it more likely to pick up 'spikes'.
    let abs = |v: i64| if v < 0 { v.wrapping_neg() } else { v };
    if abs(pt1.x.wrapping_sub(pt2.x)) > abs(pt1.y.wrapping_sub(pt2.y)) {
        if (pt1.x > pt2.x) == (pt1.x < pt3.x) {
            distance_from_line_sqrd(pt1, pt2, pt3) < dist_sqrd
        } else if (pt2.x > pt1.x) == (pt2.x < pt3.x) {
            distance_from_line_sqrd(pt2, pt1, pt3) < dist_sqrd
        } else {
            distance_from_line_sqrd(pt3, pt1, pt2) < dist_sqrd
        }
    } else if (pt1.y > pt2.y) == (pt1.y < pt3.y) {
        distance_from_line_sqrd(pt1, pt2, pt3) < dist_sqrd
    } else if (pt2.y > pt1.y) == (pt2.y < pt3.y) {
        distance_from_line_sqrd(pt2, pt1, pt3) < dist_sqrd
    } else {
        distance_from_line_sqrd(pt3, pt1, pt2) < dist_sqrd
    }
}

fn points_are_close(pt1: IntPoint, pt2: IntPoint, dist_sqrd: f64) -> bool {
    let dx = pt1.x as f64 - pt2.x as f64;
    let dy = pt1.y as f64 - pt2.y as f64;
    (dx * dx) + (dy * dy) <= dist_sqrd
}

/// Removes vertices which are closer than `distance` to their neighbors or
/// to the line through their neighbors (upstream default distance: 1.415).
/// Returns an empty path if less than 3 vertices remain.
pub fn clean_polygon(in_poly: &[IntPoint], distance: f64) -> Path {
    struct Pt {
        pt: IntPoint,
        next: usize,
        prev: usize,
        idx: i32,
    }

    let mut size = in_poly.len();
    if size == 0 {
        return Path::new();
    }

    let mut out_pts: Vec<Pt> = in_poly
        .iter()
        .enumerate()
        .map(|(i, &pt)| Pt {
            pt,
            next: (i + 1) % size,
            prev: (i + size - 1) % size,
            idx: 0,
        })
        .collect();

    fn exclude_op(pts: &mut [Pt], op: usize) -> usize {
        let result = pts[op].prev;
        let next = pts[op].next;
        pts[result].next = next;
        pts[next].prev = result;
        pts[result].idx = 0;
        result
    }

    let dist_sqrd = distance * distance;
    let mut op = 0;
    while out_pts[op].idx == 0 && out_pts[op].next != out_pts[op].prev {
        let (prev, next) = (out_pts[op].prev, out_pts[op].next);
        if points_are_close(out_pts[op].pt, out_pts[prev].pt, dist_sqrd) {
            op = exclude_op(&mut out_pts, op);
            size = size.saturating_sub(1);
        } else if points_are_close(out_pts[prev].pt, out_pts[next].pt, dist_sqrd) {
            exclude_op(&mut out_pts, next);
            op = exclude_op(&mut out_pts, op);
            size = size.saturating_sub(2);
        } else if slopes_near_collinear(
            out_pts[prev].pt,
            out_pts[op].pt,
            out_pts[next].pt,
            dist_sqrd,
        ) {
            op = exclude_op(&mut out_pts, op);
            size = size.saturating_sub(1);
        } else {
            out_pts[op].idx = 1;
            op = next;
        }
    }

    if size < 3 {
        size = 0;
    }
    let mut out_poly = Path::with_capacity(size);
    for _ in 0..size {
        out_poly.push(out_pts[op].pt);
        op = out_pts[op].next;
    }
    out_poly
}

/// Applies [`clean_polygon()`] to every polygon.
pub fn clean_polygons(in_polys: &[Path], distance: f64) -> Paths {
    in_polys
        .iter()
        .map(|p| clean_polygon(p, distance))
        .collect()
}

fn minkowski(poly: &[IntPoint], path: &[IntPoint], is_sum: bool, is_closed: bool) -> Paths {
    let delta = usize::from(is_closed);
    let poly_cnt = poly.len();
    let path_cnt = path.len();
    let pp: Paths = path
        .iter()
        .map(|pa| {
            poly.iter()
                .map(|po| {
                    if is_sum {
                        IntPoint::new(pa.x.wrapping_add(po.x), pa.y.wrapping_add(po.y))
                    } else {
                        IntPoint::new(pa.x.wrapping_sub(po.x), pa.y.wrapping_sub(po.y))
                    }
                })
                .collect()
        })
        .collect();

    let count = (path_cnt + delta).saturating_sub(1);
    let mut solution = Paths::with_capacity(count * poly_cnt);
    for i in 0..count {
        for j in 0..poly_cnt {
            let mut quad = vec![
                pp[i % path_cnt][j % poly_cnt],
                pp[(i + 1) % path_cnt][j % poly_cnt],
                pp[(i + 1) % path_cnt][(j + 1) % poly_cnt],
                pp[i % path_cnt][(j + 1) % poly_cnt],
            ];
            if !orientation(&quad) {
                reverse_path(&mut quad);
            }
            solution.push(quad);
        }
    }
    solution
}

/// Returns the Minkowski sum of `pattern` and `path`.
pub fn minkowski_sum(
    pattern: &[IntPoint],
    path: &[IntPoint],
    path_is_closed: bool,
) -> Result<Paths> {
    let solution = minkowski(pattern, path, true, path_is_closed);
    let mut c = Clipper::new();
    c.add_paths(&solution, PolyType::Subject, true)?;
    c.execute(
        ClipType::Union,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    )
}

/// Returns the Minkowski sum of `pattern` and each of `paths`.
pub fn minkowski_sum_paths(
    pattern: &[IntPoint],
    paths: &[Path],
    path_is_closed: bool,
) -> Result<Paths> {
    let mut c = Clipper::new();
    for path in paths {
        let tmp = minkowski(pattern, path, true, path_is_closed);
        c.add_paths(&tmp, PolyType::Subject, true)?;
        if path_is_closed && let Some(delta) = pattern.first() {
            let tmp2: Path = path
                .iter()
                .map(|p| IntPoint::new(p.x.wrapping_add(delta.x), p.y.wrapping_add(delta.y)))
                .collect();
            c.add_path(&tmp2, PolyType::Clip, true)?;
        }
    }
    c.execute(
        ClipType::Union,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    )
}

/// Returns the Minkowski difference of `poly1` and `poly2`.
pub fn minkowski_diff(poly1: &[IntPoint], poly2: &[IntPoint]) -> Result<Paths> {
    let solution = minkowski(poly1, poly2, false, true);
    let mut c = Clipper::new();
    c.add_paths(&solution, PolyType::Subject, true)?;
    c.execute(
        ClipType::Union,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    )
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum NodeType {
    Any,
    Closed,
}

fn add_poly_node_to_paths(tree: &PolyTree, id: NodeId, node_type: NodeType, paths: &mut Paths) {
    let data = tree.data(id);
    let matches = node_type == NodeType::Any || !data.is_open;
    if !data.contour.is_empty() && matches {
        paths.push(data.contour.clone());
    }
    for &child in &data.children {
        add_poly_node_to_paths(tree, NodeId::Node(child), node_type, paths);
    }
}

/// Returns all contours of the tree in depth-first order.
pub fn poly_tree_to_paths(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::with_capacity(polytree.total());
    add_poly_node_to_paths(polytree, NodeId::Root, NodeType::Any, &mut paths);
    paths
}

/// Returns all closed contours of the tree in depth-first order.
pub fn closed_paths_from_poly_tree(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::with_capacity(polytree.total());
    add_poly_node_to_paths(polytree, NodeId::Root, NodeType::Closed, &mut paths);
    paths
}

/// Returns all open contours of the tree (they are top-level only).
pub fn open_paths_from_poly_tree(polytree: &PolyTree) -> Paths {
    polytree
        .children()
        .filter(|n| n.is_open())
        .map(|n| n.contour().clone())
        .collect()
}
