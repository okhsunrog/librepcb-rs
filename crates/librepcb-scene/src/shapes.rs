//! Shape helpers shared by the scene builders, following upstream
//! `GraphicsPainter` (libs/librepcb/core/export/graphicspainter.{h,cpp}):
//! round pens with a minimum line width, polygons whose zero-length paths
//! become dots, and optional fills (filled shapes or grab areas).

use librepcb_canvas::kurbo::{Affine, BezPath, Circle as KCircle, Line, Point as KPoint};
use librepcb_canvas::{Brush, Geometry, Item, LayerId, StrokeStyle, Style, convert};
use librepcb_core::geometry::Path;
use librepcb_core::types::{Length, Point};

/// Minimum line width of upstream's graphics export (0.1 mm,
/// `GraphicsExportSettings::getMinLineWidth()`).
pub(crate) const MIN_LINE_WIDTH: f64 = 0.1;

/// Upstream `getPenWidthPx()`: the line width, at least [`MIN_LINE_WIDTH`].
pub(crate) fn pen(width: Length) -> f64 {
    convert::length(width).max(MIN_LINE_WIDTH)
}

/// A fill of a shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fill {
    /// Not filled.
    None,
    /// Filled with the layer color.
    Layer,
    /// Filled with a fixed color (e.g. a grab area).
    Solid(librepcb_canvas::peniko::Color),
}

impl Fill {
    fn brush(self) -> Option<Brush> {
        match self {
            Fill::None => None,
            Fill::Layer => Some(Brush::Layer),
            Fill::Solid(c) => Some(Brush::Solid(c)),
        }
    }
}

/// Upstream `drawPolygon()`: a path (transformed by `xf`) with line and
/// fill. Zero-length paths become a dot of the line width.
pub(crate) fn polygon(
    layer: LayerId,
    path: &Path,
    xf: Affine,
    line_width: Length,
    fill: Fill,
) -> Option<Item> {
    let first = path.vertices().first()?;
    if path.is_zero_length() {
        let center = xf * convert::point(first.pos);
        let circle = KCircle::new(center, convert::length(line_width) / 2.0);
        return Some(Item::new(layer, circle, Style::fill()));
    }
    let closed = path.is_closed();
    let fill = if closed { fill } else { Fill::None };
    let draw_line = line_width > Length::ZERO || fill == Fill::None;
    let style = Style {
        fill: fill.brush(),
        stroke: draw_line.then(|| StrokeStyle::new(pen(line_width))),
    };
    Some(Item::new(layer, xf * convert::path(path), style))
}

/// Upstream `drawCircle()`.
pub(crate) fn circle(
    layer: LayerId,
    center: KPoint,
    diameter: Length,
    line_width: Length,
    fill: Fill,
) -> Item {
    let draw_line = line_width > Length::ZERO || fill == Fill::None;
    let style = Style {
        fill: fill.brush(),
        stroke: draw_line.then(|| StrokeStyle::new(pen(line_width))),
    };
    Item::new(
        layer,
        KCircle::new(center, convert::length(diameter) / 2.0),
        style,
    )
}

/// Upstream `drawLine()`: a line with round caps (a dot if both ends are
/// equal).
pub(crate) fn line(layer: LayerId, p1: Point, p2: Point, width: Length) -> Item {
    Item::new(
        layer,
        Line::new(convert::point(p1), convert::point(p2)),
        Style::stroke(pen(width)),
    )
}

/// Filled outlines (e.g. of pads, planes or stop mask openings).
pub(crate) fn area(layer: LayerId, outlines: &[Path], xf: Affine) -> Option<Item> {
    if outlines.is_empty() {
        return None;
    }
    let path: BezPath = xf * convert::paths(outlines);
    Some(Item::new(layer, Geometry::Path(path), Style::fill()))
}

/// Hairline outlines (e.g. of holes or plane outlines).
pub(crate) fn outline(layer: LayerId, outlines: &[Path], xf: Affine) -> Option<Item> {
    if outlines.is_empty() {
        return None;
    }
    let path: BezPath = xf * convert::paths(outlines);
    Some(Item::new(layer, Geometry::Path(path), Style::stroke(0.0)))
}
