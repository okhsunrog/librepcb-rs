//! Throwaway loader for LibrePCB boards (boards/*/board.lp + project library).

use crate::scene::{Geom, Item, Kind, layer_id};
use crate::sexp::{self, Sexp};
use anyhow::{Context, Result};
use kurbo::{Affine, Arc, BezPath, Point, Shape, Vec2};
use std::collections::HashMap;
use std::path::Path;

fn read(path: &Path) -> Result<Sexp> {
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?;
    sexp::parse(&src)
}

/// File coordinates are y-up; world is y-down.
const FLIP: Affine = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);

fn vertices(node: &Sexp) -> Vec<(Point, f64)> {
    node.children("vertex")
        .filter_map(|v| {
            let (x, y) = v.xy("position")?;
            Some((Point::new(x, y), v.f("angle").unwrap_or(0.0)))
        })
        .collect()
}

/// LibrePCB path (vertices with arc angles to the next vertex) -> BezPath.
fn vertex_path(verts: &[(Point, f64)], close_ring: bool) -> BezPath {
    let mut p = BezPath::new();
    let Some(&(first, _)) = verts.first() else {
        return p;
    };
    p.move_to(first);
    let n = verts.len();
    let segs = if close_ring { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (a, ang) = verts[i];
        let b = verts[(i + 1) % n].0;
        if ang.abs() < 1e-9 || a == b {
            p.line_to(b);
            continue;
        }
        let theta = ang.to_radians();
        let chord = b - a;
        let l = chord.hypot();
        let normal = Vec2::new(-chord.y, chord.x) / l;
        let center = a.midpoint(b) + normal * ((l / 2.0) / (theta / 2.0).tan());
        let r = (a - center).hypot();
        let start = (a - center).atan2();
        let arc = Arc::new(center, (r, r), start, theta, 0.0);
        arc.to_cubic_beziers(0.001, |c1, c2, e| p.curve_to(c1, c2, e));
    }
    if close_ring || verts.first().map(|v| v.0) == verts.last().map(|v| v.0) {
        p.close_path();
    }
    p
}

fn is_closed(verts: &[(Point, f64)]) -> bool {
    verts.len() > 2 && verts.first().map(|v| v.0) == verts.last().map(|v| v.0)
}

fn mirror_layer(name: &str, flip: bool) -> String {
    if !flip {
        return name.to_string();
    }
    if let Some(r) = name.strip_prefix("top_") {
        format!("bot_{r}")
    } else if let Some(r) = name.strip_prefix("bot_") {
        format!("top_{r}")
    } else {
        name.to_string()
    }
}

struct DesignRules {
    via_drill: f64,
    ring_ratio: f64,
    ring_min: f64,
    ring_max: f64,
}

/// Pads (from footprints and from board netsegments).
fn add_pad(items: &mut Vec<Item>, pad: &Sexp, tf: Affine, flip: bool, label: String) -> Point {
    let (x, y) = pad.xy("position").unwrap_or((0.0, 0.0));
    let rot = pad.f("rotation").unwrap_or(0.0).to_radians();
    let (w, h) = pad.xy("size").unwrap_or((1.0, 1.0));
    let ratio = pad.f("radius").unwrap_or(0.0);
    let local = Affine::translate((x, y)) * Affine::rotate(rot);
    let full = FLIP * tf * local;
    let shape = pad.val("shape").unwrap_or("roundrect");
    let outline: BezPath = match shape {
        "custom" => vertex_path(&vertices(pad), true),
        "octagon" => {
            let c = w.min(h) * (1.0 - 1.0 / (1.0 + std::f64::consts::SQRT_2));
            let (hw, hh) = (w / 2.0, h / 2.0);
            let pts = [
                (-hw + c, -hh),
                (hw - c, -hh),
                (hw, -hh + c),
                (hw, hh - c),
                (hw - c, hh),
                (-hw + c, hh),
                (-hw, hh - c),
                (-hw, -hh + c),
            ];
            let mut p = BezPath::new();
            p.move_to(pts[0]);
            for q in &pts[1..] {
                p.line_to(*q);
            }
            p.close_path();
            p
        }
        s => {
            let r = if s == "round" { 1.0 } else { ratio };
            kurbo::RoundedRect::new(-w / 2.0, -h / 2.0, w / 2.0, h / 2.0, r * w.min(h) / 2.0)
                .to_path(0.001)
        }
    };
    let holes: Vec<_> = pad.children("hole").collect();
    let side_top = pad.val("side") != Some("bottom");
    let layer = if !holes.is_empty() {
        "pads".to_string()
    } else if side_top != flip {
        "top_cu".into()
    } else {
        "bot_cu".into()
    };
    items.push(Item::new(
        Kind::Pad,
        layer_id(&layer).unwrap(),
        Geom::Area {
            path: full * outline,
            fill: true,
            w: 0.0,
        },
        label.clone(),
    ));
    for hole in holes {
        add_hole(items, hole, full, "drills", format!("{label} drill"));
    }
    full * Point::ORIGIN
}

fn add_hole(items: &mut Vec<Item>, hole: &Sexp, tf: Affine, layer: &str, label: String) {
    let d = hole.f("diameter").unwrap_or(0.5);
    let verts = vertices(hole);
    let pts: Vec<Point> = if verts.is_empty() {
        vec![Point::ORIGIN]
    } else {
        verts.iter().map(|v| v.0).collect()
    };
    let segs: Vec<(Point, Point)> = if pts.len() == 1 {
        vec![(pts[0], pts[0])]
    } else {
        pts.windows(2).map(|w| (w[0], w[1])).collect()
    };
    for (a, b) in segs {
        let (a, b) = (tf * a, tf * b);
        items.push(Item::new(
            Kind::Hole,
            layer_id(layer).unwrap(),
            Geom::Seg { a, b, w: d },
            label.clone(),
        ));
    }
}

fn add_polygon(items: &mut Vec<Item>, poly: &Sexp, tf: Affine, flip: bool, kind: Kind, label: String) {
    let Some(layer) = poly.val("layer").and_then(|l| layer_id(&mirror_layer(l, flip))) else {
        return;
    };
    let verts = vertices(poly);
    if verts.len() < 2 {
        return;
    }
    let closed = is_closed(&verts);
    let fill = poly.val("fill") == Some("true") && closed;
    let w = poly.f("width").unwrap_or(0.0);
    let w = if !fill && w <= 0.0 { 0.1 } else { w };
    let path = FLIP * tf * vertex_path(&verts, false);
    items.push(Item::new(kind, layer, Geom::Area { path, fill, w }, label));
}

fn add_circle(items: &mut Vec<Item>, c: &Sexp, tf: Affine, flip: bool, label: String) {
    let Some(layer) = c.val("layer").and_then(|l| layer_id(&mirror_layer(l, flip))) else {
        return;
    };
    let (x, y) = c.xy("position").unwrap_or((0.0, 0.0));
    let d = c.f("diameter").unwrap_or(1.0);
    let fill = c.val("fill") == Some("true");
    let w = c.f("width").unwrap_or(0.0);
    let path = FLIP * tf * kurbo::Circle::new((x, y), d / 2.0).to_path(0.001);
    items.push(Item::new(
        Kind::Polygon,
        layer,
        Geom::Area {
            path,
            fill,
            w: if fill { w } else { w.max(0.05) },
        },
        label,
    ));
}

pub fn load_board(board_file: &Path) -> Result<Vec<Item>> {
    let board = read(board_file)?;
    let project = board_file
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .context("project dir")?;
    let lib = project.join("library");
    let dr = board.child("design_rules");
    let rules = DesignRules {
        via_drill: dr.and_then(|d| d.f("default_via_drill_diameter")).unwrap_or(0.3),
        ring_ratio: dr
            .and_then(|d| d.child("via_annular_ring"))
            .and_then(|r| r.f("ratio"))
            .unwrap_or(0.25),
        ring_min: dr
            .and_then(|d| d.child("via_annular_ring"))
            .and_then(|r| r.f("min"))
            .unwrap_or(0.2),
        ring_max: dr
            .and_then(|d| d.child("via_annular_ring"))
            .and_then(|r| r.f("max"))
            .unwrap_or(2.0),
    };

    let mut items = Vec::new();
    // (device uuid, footprint pad uuid) -> world position
    let mut pad_pos: HashMap<(String, String), Point> = HashMap::new();
    let mut pkg_cache: HashMap<String, Sexp> = HashMap::new();

    for dev in board.children("device") {
        let dev_uuid = dev.arg(0).unwrap_or_default().to_string();
        let lib_dev = dev.val("lib_device").unwrap_or_default();
        let fpt_uuid = dev.val("lib_footprint").unwrap_or_default();
        let (x, y) = dev.xy("position").unwrap_or((0.0, 0.0));
        let rot = dev.f("rotation").unwrap_or(0.0).to_radians();
        let flip = dev.val("flip") == Some("true");
        let mut tf = Affine::translate((x, y)) * Affine::rotate(rot);
        if flip {
            tf *= Affine::FLIP_X;
        }
        let dev_file = read(&lib.join("dev").join(lib_dev).join("device.lp"))?;
        let pkg_uuid = dev_file.val("package").context("device package")?.to_string();
        if !pkg_cache.contains_key(&pkg_uuid) {
            let pkg = read(&lib.join("pkg").join(&pkg_uuid).join("package.lp"))?;
            pkg_cache.insert(pkg_uuid.clone(), pkg);
        }
        let pkg = &pkg_cache[&pkg_uuid];
        let pad_names: HashMap<&str, &str> = pkg
            .children("pad")
            .filter_map(|p| Some((p.arg(0)?, p.val("name")?)))
            .collect();
        let Some(fpt) = pkg.children("footprint").find(|f| f.arg(0) == Some(fpt_uuid)) else {
            continue;
        };
        let short = &dev_uuid[..8.min(dev_uuid.len())];
        for pad in fpt.children("pad") {
            let pu = pad.arg(0).unwrap_or_default();
            let name = pad_names.get(pu).copied().unwrap_or("?");
            let pos = add_pad(&mut items, pad, tf, flip, format!("pad {name} of device {short}"));
            pad_pos.insert((dev_uuid.clone(), pu.to_string()), pos);
        }
        for poly in fpt.children("polygon") {
            add_polygon(&mut items, poly, tf, flip, Kind::Polygon, format!("footprint polygon of {short}"));
        }
        for c in fpt.children("circle") {
            add_circle(&mut items, c, tf, flip, format!("footprint circle of {short}"));
        }
        for h in fpt.children("hole") {
            add_hole(&mut items, h, FLIP * tf, "holes", format!("footprint hole of {short}"));
        }
    }

    for seg in board.children("netsegment") {
        let net = seg.val("net").unwrap_or("none");
        let net = &net[..8.min(net.len())];
        let mut anchors: HashMap<String, Point> = HashMap::new();
        for j in seg.children("junction") {
            let (x, y) = j.xy("position").unwrap_or_default();
            anchors.insert(j.arg(0).unwrap_or_default().into(), FLIP * Point::new(x, y));
        }
        for v in seg.children("via") {
            let (x, y) = v.xy("position").unwrap_or_default();
            let p = FLIP * Point::new(x, y);
            let drill = v.f("drill").unwrap_or(rules.via_drill);
            let size = v.f("size").unwrap_or_else(|| {
                drill + 2.0 * (drill * rules.ring_ratio).clamp(rules.ring_min, rules.ring_max)
            });
            let id = v.arg(0).unwrap_or_default();
            items.push(Item::new(
                Kind::Via,
                layer_id("vias").unwrap(),
                Geom::Seg { a: p, b: p, w: size },
                format!("via {} (net {net})", &id[..8.min(id.len())]),
            ));
            items.push(Item::new(
                Kind::Hole,
                layer_id("drills").unwrap(),
                Geom::Seg { a: p, b: p, w: drill },
                "via drill".into(),
            ));
            anchors.insert(id.into(), p);
        }
        for pad in seg.children("pad") {
            let id = pad.arg(0).unwrap_or_default().to_string();
            let p = add_pad(&mut items, pad, Affine::IDENTITY, false, format!("board pad (net {net})"));
            anchors.insert(id, p);
        }
        for t in seg.children("trace") {
            let Some(layer) = t.val("layer").and_then(layer_id) else {
                continue;
            };
            let w = t.f("width").unwrap_or(0.2);
            let resolve = |n: Option<&Sexp>| -> Option<Point> {
                let n = n?;
                if let Some(d) = n.val("device") {
                    return pad_pos.get(&(d.to_string(), n.val("pad")?.to_string())).copied();
                }
                for k in ["junction", "via", "pad"] {
                    if let Some(id) = n.val(k) {
                        return anchors.get(id).copied();
                    }
                }
                None
            };
            let (Some(a), Some(b)) = (resolve(t.child("from")), resolve(t.child("to"))) else {
                eprintln!("unresolved trace {:?}", t.arg(0));
                continue;
            };
            let id = t.arg(0).unwrap_or_default();
            items.push(Item::new(
                Kind::Trace,
                layer,
                Geom::Seg { a, b, w },
                format!("trace {} (net {net}, w={w})", &id[..8.min(id.len())]),
            ));
        }
    }

    for poly in board.children("polygon") {
        add_polygon(&mut items, poly, Affine::IDENTITY, false, Kind::Polygon, format!("board polygon on {}", poly.val("layer").unwrap_or("?")));
    }
    for plane in board.children("plane") {
        let Some(layer) = plane.val("layer").and_then(layer_id) else {
            continue;
        };
        let verts = vertices(plane);
        let path = FLIP * vertex_path(&verts, !is_closed(&verts));
        items.push(Item::new(
            Kind::Plane,
            layer,
            Geom::Area { path, fill: false, w: 0.2 },
            format!("plane outline on {}", plane.val("layer").unwrap_or("?")),
        ));
    }
    for zone in board.children("zone") {
        let verts = vertices(zone);
        let path = FLIP * vertex_path(&verts, !is_closed(&verts));
        items.push(Item::new(
            Kind::Zone,
            layer_id("zones").unwrap(),
            Geom::Area { path, fill: true, w: 0.0 },
            "keepout zone".into(),
        ));
    }
    for hole in board.children("hole") {
        add_hole(&mut items, hole, FLIP, "holes", "board hole".into());
    }
    Ok(items)
}
