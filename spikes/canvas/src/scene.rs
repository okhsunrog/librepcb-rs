//! Renderer-agnostic scene: items with geometry in world millimetres (y axis
//! pointing down, i.e. the file's y is negated), layers with colors, grouping
//! into (layer, style, tile) batches, and R-tree based hit testing.

use kurbo::{Affine, BezPath, ParamCurveNearest, PathEl, Point, Rect, Shape, Vec2};
use rstar::{AABB, RTree, primitives::GeomWithData, primitives::Rectangle};
use std::collections::BTreeMap;

/// Draw order == index. Colors are ARGB from LibrePCB's "LibrePCB Dark"
/// board color scheme (libs/librepcb/core/workspace/basecolorscheme.cpp).
pub const LAYERS: &[(&str, u32)] = &[
    ("zones", 0x80494949),
    ("bot_courtyard", 0xc0ff00ff),
    ("bot_documentation", 0x76fbc697),
    ("bot_package_outlines", 0xc000ffff),
    ("bot_glue", 0x64e0e0e0),
    ("bot_legend", 0xbbffffff),
    ("bot_names", 0x96edffd8),
    ("bot_values", 0x96d8f2ff),
    ("bot_cu", 0x964578cc),
    ("in6_cu", 0x967b20a3),
    ("in5_cu", 0x96a70049),
    ("in4_cu", 0x96e2a1ff),
    ("in3_cu", 0x96ee5c9b),
    ("in2_cu", 0x96e50063),
    ("in1_cu", 0x96cc57ff),
    ("top_cu", 0x96cc0802),
    ("vias", 0x966db515),
    ("pads", 0x966db515),
    ("drills", 0xff000000),
    ("holes", 0xc8ffffff),
    ("top_glue", 0x64e0e0e0),
    ("top_documentation", 0x76fbc697),
    ("top_package_outlines", 0xc000ffff),
    ("top_courtyard", 0xc0ff00ff),
    ("top_legend", 0xbbffffff),
    ("top_names", 0x96edffd8),
    ("top_values", 0x96d8f2ff),
    ("brd_documentation", 0x76fbc697),
    ("brd_plated_cutouts", 0xc800ddff),
    ("brd_cutouts", 0xc8ffffff),
    ("brd_outlines", 0xc8ffffff),
];

pub const BACKGROUND: u32 = 0xff000000;
pub const HIGHLIGHT: u32 = 0xe0ffff00;

pub fn layer_id(name: &str) -> Option<u16> {
    LAYERS.iter().position(|(n, _)| *n == name).map(|i| i as u16)
}

pub fn layer_color(id: u16) -> u32 {
    LAYERS[id as usize].1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Trace,
    Via,
    Pad,
    Hole,
    Polygon,
    Plane,
    Zone,
}

#[derive(Debug, Clone)]
pub enum Geom {
    /// Round-capped line segment (a == b gives a filled circle).
    Seg { a: Point, b: Point, w: f64 },
    /// Outline, optionally filled and/or stroked (round joins).
    Area { path: BezPath, fill: bool, w: f64 },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    pub layer: u16,
    pub geom: Geom,
    pub label: String,
    pub bbox: Rect,
}

impl Item {
    pub fn new(kind: Kind, layer: u16, mut geom: Geom, label: String) -> Self {
        let bbox = match &mut geom {
            Geom::Seg { a, b, w } => Rect::from_points(*a, *b).inflate(*w / 2.0, *w / 2.0),
            Geom::Area { path, fill, w } => {
                if *fill {
                    normalize_winding(path);
                }
                path.bounding_box().inflate(*w / 2.0, *w / 2.0)
            }
        };
        Self {
            kind,
            layer,
            geom,
            label,
            bbox,
        }
    }

    /// Precise hit test in world coordinates with tolerance `tol` (mm).
    pub fn hit(&self, p: Point, tol: f64) -> bool {
        match &self.geom {
            Geom::Seg { a, b, w } => {
                let d = if a == b {
                    (p - *a).hypot()
                } else {
                    kurbo::Line::new(*a, *b).nearest(p, 1e-6).distance_sq.sqrt()
                };
                d <= w / 2.0 + tol
            }
            Geom::Area { path, fill, w } => {
                if *fill && path.contains(p) {
                    return true;
                }
                let lim = w / 2.0 + tol;
                path.segments()
                    .any(|s| s.nearest(p, 1e-6).distance_sq <= lim * lim)
            }
        }
    }
}

/// Make every closed subpath wind the same way so a non-zero fill of many
/// merged shapes is their union (mirrored footprints flip the winding).
fn normalize_winding(path: &mut BezPath) {
    let mut out = BezPath::new();
    let mut cur = BezPath::new();
    let flush = |cur: &mut BezPath, out: &mut BezPath| {
        if cur.elements().is_empty() {
            return;
        }
        if cur.area() < 0.0 {
            *cur = cur.reverse_subpaths();
        }
        out.extend(cur.elements().iter().copied());
        *cur = BezPath::new();
    };
    for el in path.elements() {
        if let PathEl::MoveTo(_) = el {
            flush(&mut cur, &mut out);
        }
        cur.push(*el);
    }
    flush(&mut cur, &mut out);
    *path = out;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Style {
    Fill,
    /// Stroke width in nanometres.
    Stroke(i64),
}

/// A batch of items sharing layer + style, restricted to one spatial tile.
pub struct Group {
    pub layer: u16,
    pub style: Style,
    pub tile: usize,
    pub items: Vec<usize>,
    pub bbox: Rect,
}

impl Group {
    pub fn stroke_width(&self) -> f64 {
        match self.style {
            Style::Fill => 0.0,
            Style::Stroke(nm) => nm as f64 * 1e-6,
        }
    }

    /// World-space path (mm) of all items in this group.
    pub fn path(&self, items: &[Item]) -> BezPath {
        let mut p = BezPath::new();
        for &i in &self.items {
            append_item_path(&mut p, &items[i], self.style);
        }
        p
    }
}

pub fn append_item_path(p: &mut BezPath, item: &Item, style: Style) {
    match (&item.geom, style) {
        (Geom::Seg { a, b, .. }, _) => {
            p.move_to(*a);
            // Tiny offset so zero-length round caps render everywhere.
            // (1 um: must survive the integer-um SVG encoding; femtovg drops
            // zero-length round-capped subpaths entirely.)
            p.line_to(if a == b { *a + Vec2::new(1e-3, 0.0) } else { *b });
        }
        (Geom::Area { path, .. }, _) => p.extend(path.elements().iter().copied()),
    }
}

pub struct Scene {
    pub name: String,
    pub items: Vec<Item>,
    pub bbox: Rect,
    pub groups: Vec<Group>,
    pub rtree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>>,
}

impl Scene {
    pub fn new(name: String, items: Vec<Item>, tiles: usize) -> Self {
        let bbox = items
            .iter()
            .map(|i| i.bbox)
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
        let rtree = RTree::bulk_load(
            items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    GeomWithData::new(
                        Rectangle::from_corners([it.bbox.x0, it.bbox.y0], [it.bbox.x1, it.bbox.y1]),
                        i,
                    )
                })
                .collect(),
        );
        let mut s = Self {
            name,
            items,
            bbox,
            groups: Vec::new(),
            rtree,
        };
        s.regroup(tiles);
        s
    }

    /// Batch items by (layer, style, tile). `tiles` is the grid size per axis.
    pub fn regroup(&mut self, tiles: usize) {
        let tiles = tiles.max(1);
        let mut map: BTreeMap<(u16, Style, usize), Vec<usize>> = BTreeMap::new();
        let (w, h) = (self.bbox.width().max(1e-9), self.bbox.height().max(1e-9));
        for (i, it) in self.items.iter().enumerate() {
            let c = it.bbox.center();
            let tx = (((c.x - self.bbox.x0) / w * tiles as f64) as usize).min(tiles - 1);
            let ty = (((c.y - self.bbox.y0) / h * tiles as f64) as usize).min(tiles - 1);
            let tile = ty * tiles + tx;
            let styles: &[Style] = match &it.geom {
                Geom::Seg { w, .. } => &[Style::Stroke((w * 1e6).round() as i64)],
                Geom::Area { fill, w, .. } => match (*fill, *w > 0.0) {
                    (true, true) => &[Style::Fill, Style::Stroke((w * 1e6).round() as i64)],
                    (true, false) => &[Style::Fill],
                    (false, _) => &[Style::Stroke((w.max(0.05) * 1e6).round() as i64)],
                },
            };
            for st in styles {
                map.entry((it.layer, *st, tile)).or_default().push(i);
            }
        }
        self.groups = map
            .into_iter()
            .map(|((layer, style, tile), items)| {
                let bbox = items
                    .iter()
                    .map(|&i| self.items[i].bbox)
                    .reduce(|a, b| a.union(b))
                    .unwrap();
                Group {
                    layer,
                    style,
                    tile,
                    items,
                    bbox,
                }
            })
            .collect();
    }

    /// Returns the topmost item under `p` (world mm).
    pub fn hit_test(&self, p: Point, tol: f64) -> Option<usize> {
        let env = AABB::from_corners([p.x - tol, p.y - tol], [p.x + tol, p.y + tol]);
        self.rtree
            .locate_in_envelope_intersecting(env)
            .map(|g| g.data)
            .filter(|&i| self.items[i].hit(p, tol))
            .max_by_key(|&i| (self.items[i].layer, i))
    }

    pub fn stats(&self) -> String {
        let count = |k: Kind| self.items.iter().filter(|i| i.kind == k).count();
        format!(
            "{}: {} items ({} traces, {} pads, {} vias, {} holes, {} polygons), {} groups, bbox {:.1}x{:.1} mm",
            self.name,
            self.items.len(),
            count(Kind::Trace),
            count(Kind::Pad),
            count(Kind::Via),
            count(Kind::Hole),
            count(Kind::Polygon) + count(Kind::Plane) + count(Kind::Zone),
            self.groups.len(),
            self.bbox.width(),
            self.bbox.height()
        )
    }
}

/// View transform: screen_px = world_mm * zoom + pan  (logical px).
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub zoom: f64,
    pub pan: Vec2,
}

impl View {
    pub fn fit(bbox: Rect, w: f64, h: f64) -> Self {
        let zoom = (w / bbox.width()).min(h / bbox.height()) * 0.95;
        let pan = Vec2::new(w / 2.0, h / 2.0) - bbox.center().to_vec2() * zoom;
        Self { zoom, pan }
    }
    pub fn affine(&self, scale_factor: f64) -> Affine {
        Affine::scale(scale_factor) * Affine::translate(self.pan) * Affine::scale(self.zoom)
    }
    pub fn to_world(&self, sx: f64, sy: f64) -> Point {
        ((Vec2::new(sx, sy) - self.pan) / self.zoom).to_point()
    }
    pub fn zoom_at(&mut self, sx: f64, sy: f64, factor: f64) {
        let w = self.to_world(sx, sy);
        self.zoom *= factor;
        self.pan = Vec2::new(sx, sy) - w.to_vec2() * self.zoom;
    }
    /// Visible world rect for a viewport of w x h logical px.
    pub fn visible(&self, w: f64, h: f64) -> Rect {
        Rect::from_points(self.to_world(0.0, 0.0), self.to_world(w, h))
    }
}

// ---------------------------------------------------------------------------
// Synthetic stress scene
// ---------------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.next() % v.len() as u64) as usize]
    }
}

/// ~`traces` traces on 4 copper layers, `pads` SMT pads, `vias` vias.
pub fn stress(traces: usize, pads: usize, vias: usize) -> Vec<Item> {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (bw, bh) = (300.0, 200.0);
    let mut items = Vec::new();
    let mut outline = BezPath::new();
    outline.move_to((0.0, 0.0));
    outline.line_to((bw, 0.0));
    outline.line_to((bw, bh));
    outline.line_to((0.0, bh));
    outline.close_path();
    items.push(Item::new(
        Kind::Polygon,
        layer_id("brd_outlines").unwrap(),
        Geom::Area {
            path: outline,
            fill: false,
            w: 0.0,
        },
        "outline".into(),
    ));
    let cu = ["top_cu", "bot_cu", "in1_cu", "in2_cu"].map(|n| layer_id(n).unwrap());
    let mut anchors = Vec::new();
    for i in 0..pads {
        let c = Point::new(2.0 + rng.f() * (bw - 4.0), 2.0 + rng.f() * (bh - 4.0));
        let (w, h): (f64, f64) = (rng.pick(&[0.6, 1.0, 1.5]), rng.pick(&[0.3, 0.6, 0.8]));
        let rr = Rect::from_center_size(c, (w, h)).to_rounded_rect(w.min(h) * 0.25);
        let rot = Affine::rotate_about(rng.pick(&[0.0, std::f64::consts::FRAC_PI_2]), c);
        items.push(Item::new(
            Kind::Pad,
            rng.pick(&[cu[0], cu[1]]),
            Geom::Area {
                path: rot * rr.to_path(0.01),
                fill: true,
                w: 0.0,
            },
            format!("pad #{i}"),
        ));
        anchors.push(c);
    }
    for i in 0..vias {
        let c = Point::new(2.0 + rng.f() * (bw - 4.0), 2.0 + rng.f() * (bh - 4.0));
        items.push(Item::new(
            Kind::Via,
            layer_id("vias").unwrap(),
            Geom::Seg { a: c, b: c, w: 0.6 },
            format!("via #{i}"),
        ));
        items.push(Item::new(
            Kind::Hole,
            layer_id("drills").unwrap(),
            Geom::Seg { a: c, b: c, w: 0.3 },
            format!("via #{i} drill"),
        ));
        anchors.push(c);
    }
    let dirs: Vec<Vec2> = (0..8)
        .map(|k| Vec2::from_angle(k as f64 * std::f64::consts::FRAC_PI_4))
        .collect();
    let mut n = 0;
    while n < traces {
        let mut p = anchors[(rng.next() % anchors.len() as u64) as usize];
        let layer = rng.pick(&cu);
        let w = rng.pick(&[0.15, 0.2, 0.25, 0.4]);
        for _ in 0..8 {
            let d = rng.pick(&dirs) * (0.5 + rng.f() * 4.5);
            let q = Point::new((p.x + d.x).clamp(0.5, bw - 0.5), (p.y + d.y).clamp(0.5, bh - 0.5));
            items.push(Item::new(
                Kind::Trace,
                layer,
                Geom::Seg { a: p, b: q, w },
                format!("trace #{n}"),
            ));
            p = q;
            n += 1;
        }
    }
    items
}
