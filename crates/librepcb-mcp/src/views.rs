//! JSON views of model objects shared by several tools (millimeters and
//! degrees, see [`units`](crate::units)).

use librepcb_core::attribute::AttributeList;
use librepcb_core::geometry::{Pad, Path};
use librepcb_core::types::{Angle, Point};
use serde_json::{Value, json};

use crate::units::{PointMm, deg, mm};

/// A point as `{"x": mm, "y": mm}`.
pub fn point(p: Point) -> Value {
    json!(PointMm::from(p))
}

/// An angle in degrees.
pub fn angle(a: Angle) -> Value {
    json!(deg(a))
}

/// A path as a list of vertices `{"x", "y"}` (plus `"arc_deg"` for arc
/// segments starting at the vertex).
pub fn path(path: &Path) -> Value {
    Value::Array(
        path.vertices()
            .iter()
            .map(|v| {
                let mut o = json!({ "x": mm(v.pos.x), "y": mm(v.pos.y) });
                if v.angle.to_micro_deg() != 0 {
                    o["arc_deg"] = json!(deg(v.angle));
                }
                o
            })
            .collect(),
    )
}

/// Attributes as `[{"key", "value", "type", "unit"}]`.
pub fn attributes(list: &AttributeList) -> Value {
    Value::Array(
        list.iter()
            .map(|a| {
                json!({
                    "key": a.key().as_str(),
                    "value": a.value(),
                    "type": a.attribute_type().name(),
                    "unit": a.unit().map(|u| u.name()),
                })
            })
            .collect(),
    )
}

/// The geometry of a pad (relative to its footprint, or absolute for
/// placed pads when `position`/`rotation` are the placed values).
pub fn pad_geometry(pad: &Pad, position: Point, rotation: Angle) -> Value {
    let holes: Vec<Value> = pad
        .holes()
        .iter()
        .map(|h| {
            json!({
                "diameter": mm(h.diameter()),
                "slot": h.is_slot(),
            })
        })
        .collect();
    json!({
        "position": point(position),
        "rotation": angle(rotation),
        "shape": pad.shape().to_str(),
        "width": mm(pad.width()),
        "height": mm(pad.height()),
        "tht": pad.is_tht(),
        "side": if pad.is_tht() { "tht" } else { pad.component_side().to_str() },
        "function": pad.function().to_str(),
        "holes": holes,
    })
}

/// Natural, case-insensitive string order ("R2" < "R10"), like the
/// upstream `Toolbox::sortNumeric()`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    alphanumeric_sort::compare_str(a.to_lowercase(), b.to_lowercase())
}
