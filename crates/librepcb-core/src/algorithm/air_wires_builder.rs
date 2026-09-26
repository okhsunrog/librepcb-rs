//! Port of libs/librepcb/core/algorithm/airwiresbuilder.{h,cpp}.
//!
//! Upstream computes candidate edges with a patched copy of the
//! `delaunay-triangulation` library (plus fallback edges because that
//! library is not robust for collinear/duplicate points) and then runs
//! Kruskal's algorithm. Here, the Delaunay triangulation comes from
//! [`spade`] (exact predicates, robust for degenerate input) and the
//! minimum spanning forest from [`petgraph`]:
//!
//! - Candidate edges are the Delaunay edges, weighted by the squared
//!   distance (as `f64`, like upstream). Duplicate points (which the
//!   triangulation merges) are connected by zero-length edges.
//! - Already connected edges get a weight lower than any candidate, so
//!   Kruskal takes them first; the air wires are the candidate edges of the
//!   resulting minimum spanning forest.
//!
//! The Euclidean minimum spanning tree is a subgraph of the Delaunay
//! triangulation, so the result is the same as upstream, except that among
//! several equally long alternatives a different one may be chosen (which
//! upstream leaves to the unstable `std::sort`).

use petgraph::algo::min_spanning_tree;
use petgraph::data::Element;
use petgraph::graph::{NodeIndex, UnGraph};
use spade::handles::FixedVertexHandle;
use spade::{DelaunayTriangulation, Point2, Triangulation};

use crate::types::Point;

/// An air wire between two points, identified by the indices returned by
/// [`AirWiresBuilder::add_point()`].
pub type AirWire = (usize, usize);

/// Weight of an edge in the graph; connected edges sort before any air wire
/// candidate.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Weight {
    /// Already connected (e.g. by a trace).
    Connected,
    /// Air wire candidate with its squared length.
    Candidate(f64),
}

/// Calculates the air wires which connect a set of points, taking already
/// existing connections into account.
#[derive(Debug, Clone, Default)]
pub struct AirWiresBuilder {
    points: Vec<Point>,
    edges: Vec<(usize, usize)>,
}

impl AirWiresBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a point and returns its index.
    pub fn add_point(&mut self, point: Point) -> usize {
        self.points.push(point);
        self.points.len() - 1
    }

    /// Adds an existing connection between two points.
    ///
    /// # Panics
    ///
    /// [`build_air_wires()`](Self::build_air_wires) panics if an index was
    /// not returned by [`add_point()`](Self::add_point).
    pub fn add_edge(&mut self, p1: usize, p2: usize) {
        debug_assert!(p1 < self.points.len() && p2 < self.points.len());
        self.edges.push((p1, p2));
    }

    /// Returns the air wires needed to connect all points with the shortest
    /// total length (in no particular order).
    pub fn build_air_wires(&self) -> Vec<AirWire> {
        let mut graph = UnGraph::<(), Weight, usize>::with_capacity(self.points.len(), 0);
        for _ in &self.points {
            graph.add_node(());
        }
        let node = NodeIndex::new;
        for &(p1, p2) in &self.edges {
            graph.add_edge(node(p1), node(p2), Weight::Connected);
        }
        for (p1, p2) in self.candidate_edges() {
            let weight = dist2(self.points[p1], self.points[p2]);
            graph.add_edge(node(p1), node(p2), Weight::Candidate(weight));
        }
        min_spanning_tree(&graph)
            .filter_map(|element| match element {
                Element::Edge {
                    source,
                    target,
                    weight: Weight::Candidate(_),
                } => Some((source, target)),
                _ => None,
            })
            .collect()
    }

    /// Returns the Delaunay edges, plus zero-length edges between duplicate
    /// points.
    fn candidate_edges(&self) -> Vec<(usize, usize)> {
        let mut edges = Vec::new();
        let mut triangulation = DelaunayTriangulation::<Point2<f64>>::new();
        // Index of the first point inserted as each triangulation vertex.
        let mut vertex_points: Vec<usize> = Vec::with_capacity(self.points.len());
        for (index, point) in self.points.iter().enumerate() {
            let position = Point2::new(point.x.to_nm() as f64, point.y.to_nm() as f64);
            match triangulation.insert(position) {
                Ok(vertex) if vertex.index() < vertex_points.len() => {
                    // Duplicate position: the existing vertex was returned.
                    edges.push((vertex_points[vertex.index()], index));
                }
                Ok(_) => vertex_points.push(index),
                Err(_) => {
                    // Unreachable for integer coordinates (errors are only
                    // returned for NaN, infinite or subnormal values). Keep
                    // the point connected anyway, like upstream's fallback.
                    if let Some(previous) = index.checked_sub(1) {
                        edges.push((previous, index));
                    }
                }
            }
        }
        let point_of = |vertex: FixedVertexHandle| vertex_points[vertex.index()];
        edges.extend(triangulation.undirected_edges().map(|edge| {
            let [v1, v2] = edge.vertices();
            (point_of(v1.fix()), point_of(v2.fix()))
        }));
        edges
    }
}

/// Squared distance, calculated like upstream (`delaunay::Vector2::dist2()`).
fn dist2(p1: Point, p2: Point) -> f64 {
    let dx = p1.x.to_nm() as f64 - p2.x.to_nm() as f64;
    let dy = p1.y.to_nm() as f64 - p2.y.to_nm() as f64;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut airwires: Vec<AirWire>) -> Vec<AirWire> {
        for (a, b) in &mut airwires {
            if b < a {
                std::mem::swap(a, b);
            }
        }
        airwires.sort();
        airwires
    }

    #[test]
    fn grid_with_duplicates() {
        // 4x4 grid (co-circular points everywhere) with duplicated corner.
        let mut builder = AirWiresBuilder::new();
        for y in 0..4 {
            for x in 0..4 {
                builder.add_point(Point::from_nm(x * 1000, y * 1000));
            }
        }
        let dup = builder.add_point(Point::from_nm(0, 0));
        builder.add_edge(0, 1);
        let airwires = sorted(builder.build_air_wires());
        assert_eq!(airwires.len(), 15);
        assert!(airwires.contains(&(0, dup)));
        assert!(!airwires.contains(&(0, 1)));
        // All air wires have the grid spacing (or zero) as length.
        for (a, b) in airwires {
            let d = dist2(builder.points[a], builder.points[b]);
            assert!(d == 0.0 || d == 1e6, "{a}-{b}: {d}");
        }
    }

    #[test]
    fn repeated_calls() {
        let mut builder = AirWiresBuilder::new();
        builder.add_point(Point::from_nm(0, 0));
        builder.add_point(Point::from_nm(10, 0));
        builder.add_point(Point::from_nm(0, 10));
        builder.add_point(Point::from_nm(10, 10));
        let first = sorted(builder.build_air_wires());
        assert_eq!(first.len(), 3);
        assert_eq!(first, sorted(builder.build_air_wires()));
    }
}
