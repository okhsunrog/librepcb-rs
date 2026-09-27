//! Automatic placement of symbols in schematics and devices on boards
//! (no upstream counterpart: upstream places interactively at the cursor).
//! Used by agents which add parts without coordinates.
//!
//! Hand-written simple first-fit packing on a grid: the bounding
//! rectangle of the item (library symbol graphics and pins, or footprint
//! courtyard and pads) is moved over candidate positions row by row until
//! it overlaps no other item (plus a margin). No crate implements this
//! against LibrePCB geometry; the algorithm is intentionally simple and
//! deterministic.

use std::collections::BTreeSet;

use librepcb_core::library::pkg::Footprint;
use librepcb_core::library::sym::Symbol;
use librepcb_core::project::board::{Board, BoardDevice};
use librepcb_core::project::{BoardId, ComponentInstanceId, Project, SchematicId, SymbolId};
use librepcb_core::types::{Angle, Layer, Length, Point, PositiveLength, UnsignedLength};
use librepcb_core::utils::transform::Transform;
use librepcb_i18n::tr;

use super::component::{AddDevice, MoveDevice, MoveSymbol};
use super::{ComponentRef, resolve};
use crate::editor::{Command, Transaction};
use crate::error::{Error, Result};

/// An axis aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    /// Bottom left corner.
    pub min: Point,
    /// Top right corner.
    pub max: Point,
}

impl Rect {
    /// The rectangle containing all `points`, if any.
    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let mut r = Rect {
            min: first,
            max: first,
        };
        for p in it {
            r.min.x = r.min.x.min(p.x);
            r.min.y = r.min.y.min(p.y);
            r.max.x = r.max.x.max(p.x);
            r.max.y = r.max.y.max(p.y);
        }
        Some(r)
    }

    /// Width.
    pub fn width(&self) -> Length {
        self.max.x - self.min.x
    }

    /// Height.
    pub fn height(&self) -> Length {
        self.max.y - self.min.y
    }

    /// The rectangle grown by `margin` on each side.
    pub fn expanded(&self, margin: Length) -> Self {
        Rect {
            min: Point::new(self.min.x - margin, self.min.y - margin),
            max: Point::new(self.max.x + margin, self.max.y + margin),
        }
    }

    /// The rectangle moved by `offset`.
    pub fn translated(&self, offset: Point) -> Self {
        Rect {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Whether the rectangles overlap (touching edges do not overlap).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }

    /// Whether `other` lies completely inside this rectangle.
    pub fn contains(&self, other: &Rect) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && other.max.x <= self.max.x
            && other.max.y <= self.max.y
    }

    fn corners(&self) -> [Point; 4] {
        [
            self.min,
            Point::new(self.max.x, self.min.y),
            self.max,
            Point::new(self.min.x, self.max.y),
        ]
    }

    /// The bounding rectangle of this rectangle mapped by `transform`.
    pub(crate) fn mapped(&self, transform: &Transform) -> Self {
        // A rectangle has 4 corners, so the bounding rect always exists.
        Rect::bounding(self.corners().map(|c| transform.map(&c))).unwrap_or(*self)
    }
}

/// Rounds `v` to a multiple of `grid`.
fn snap(v: Length, grid: Length) -> Length {
    let g = grid.to_nm().max(1);
    let n = v.to_nm();
    Length::new((n as f64 / g as f64).round() as i64 * g)
}

/// Bounding rectangle of a library symbol in symbol coordinates: polygons,
/// circles, pins (with their length) and, if `with_texts`, the text
/// anchors.
pub fn library_symbol_rect(symbol: &Symbol, with_texts: bool) -> Rect {
    let mut points = Vec::new();
    for polygon in symbol.polygons().iter() {
        points.extend(polygon.path().vertices().iter().map(|v| v.pos));
    }
    for circle in symbol.circles().iter() {
        let r = *circle.diameter() / 2;
        points.push(Point::new(circle.center().x - r, circle.center().y - r));
        points.push(Point::new(circle.center().x + r, circle.center().y + r));
    }
    for pin in symbol.pins().iter() {
        points.push(pin.position());
        points.push(
            Point::new(pin.position().x + *pin.length(), pin.position().y)
                .rotated(pin.rotation(), pin.position()),
        );
    }
    if with_texts {
        for text in symbol.texts().iter() {
            points.push(text.position());
        }
    }
    Rect::bounding(points).unwrap_or(Rect {
        min: Point::new(Length::new(-2_540_000), Length::new(-2_540_000)),
        max: Point::new(Length::new(2_540_000), Length::new(2_540_000)),
    })
}

/// Bounding rectangle of a placed symbol in schematic coordinates (see
/// [`library_symbol_rect()`]).
pub(crate) fn placed_symbol_rect(
    p: &Project,
    schematic: SchematicId,
    symbol: SymbolId,
    with_texts: bool,
) -> Result<Rect> {
    let s = p
        .schematic(schematic)
        .ok_or_else(|| Error::not_found("Schematic", schematic))?;
    let sym = s
        .symbols()
        .get(&symbol)
        .ok_or_else(|| Error::not_found("Symbol", symbol))?;
    let resolved = sym.resolve(p.view())?;
    Ok(library_symbol_rect(resolved.lib_symbol, with_texts).mapped(&sym.transform()))
}

/// Symbols larger than this (in both dimensions) are considered frames and
/// ignored as obstacles.
const FRAME_SIZE: (Length, Length) = (Length::new(150_000_000), Length::new(100_000_000));

/// Obstacles on a schematic page: all symbols (except `exclude` and
/// frames), net lines, junctions and net labels.
pub(crate) fn schematic_obstacles(
    p: &Project,
    schematic: SchematicId,
    exclude: &BTreeSet<SymbolId>,
) -> Result<Vec<Rect>> {
    let s = p
        .schematic(schematic)
        .ok_or_else(|| Error::not_found("Schematic", schematic))?;
    let mut rects = Vec::new();
    for id in s.symbols().keys() {
        if exclude.contains(id) {
            continue;
        }
        let r = placed_symbol_rect(p, schematic, *id, true)?;
        if r.width() > FRAME_SIZE.0 && r.height() > FRAME_SIZE.1 {
            continue;
        }
        rects.push(r);
    }
    let label = Point::new(Length::new(10_160_000), Length::new(2_540_000));
    for (id, segment) in s.net_segments() {
        for line in segment.lines().values() {
            let ends = [line.p1(), line.p2()]
                .map(|a| s.net_line_anchor_position(*id, a, p.view()))
                .into_iter()
                .flatten();
            if let Some(r) = Rect::bounding(ends) {
                rects.push(r);
            }
        }
        for junction in segment.junctions().values() {
            rects.push(Rect {
                min: junction.position(),
                max: junction.position(),
            });
        }
        for l in segment.labels().values() {
            let r = Rect {
                min: Point::new(Length::ZERO, Length::ZERO),
                max: label,
            }
            .mapped(&Transform::new(l.position(), l.rotation(), l.mirrored()));
            rects.push(r);
        }
    }
    Ok(rects)
}

/// Area scanned for free schematic positions: from the top left corner
/// (x, y) to the right by the width, rows downwards.
const SCHEMATIC_AREA: (Length, Length, Length) = (
    Length::new(20_320_000),
    Length::new(160_020_000),
    Length::new(254_000_000),
);

/// Returns the first free position (symbol origin, on the 2.54 mm grid)
/// for a symbol whose rect relative to its origin is `local`, scanning
/// rows from the top left of the schematic area.
fn free_position(local: Rect, obstacles: &[Rect], margin: Length) -> Point {
    let grid = Length::new(2_540_000);
    let step = Length::new(5_080_000);
    let (left, top, width) = SCHEMATIC_AREA;
    let mut y = top;
    // Scan at most ~2.5 m downwards; afterwards place below everything.
    for _ in 0..500 {
        let mut x = left;
        while x + local.width() <= left + width || x == left {
            let origin = Point::new(snap(x - local.min.x, grid), snap(y - local.max.y, grid));
            let r = local.translated(origin).expanded(margin);
            if !obstacles.iter().any(|o| o.intersects(&r)) {
                return origin;
            }
            x += step;
        }
        y -= step;
    }
    let bottom = obstacles
        .iter()
        .map(|o| o.min.y)
        .min()
        .unwrap_or(Length::ZERO);
    Point::new(
        snap(left - local.min.x, grid),
        snap(bottom - margin - local.max.y, grid),
    )
}

/// Moves symbols to free positions on their schematic pages (no overlap
/// with other symbols, wires and labels), in the given order; gates of a
/// component end up next to each other. Keeps rotation and mirroring.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoPlaceSymbols {
    /// The symbols to place.
    pub symbols: Vec<SymbolId>,
    /// Free space around each symbol (default: 5.08 mm).
    #[serde(default)]
    pub margin: Option<UnsignedLength>,
}

impl Command for AutoPlaceSymbols {
    /// The new positions (in the order of the symbols).
    type Output = Vec<Point>;

    fn text(&self) -> String {
        tr!("CmdMoveSelectedSchematicItems", "Move Schematic Items")
    }

    fn execute(self, tx: &mut Transaction<'_>) -> Result<Vec<Point>> {
        let margin = self
            .margin
            .map_or(Length::new(5_080_000), |m| *m)
            .max(Length::ZERO);
        let exclude: BTreeSet<SymbolId> = self.symbols.iter().copied().collect();
        let mut positions = Vec::new();
        let mut obstacles: Vec<(SchematicId, Vec<Rect>)> = Vec::new();
        for symbol in self.symbols {
            let schematic = super::schematic::symbol_schematic(tx.project(), symbol)?;
            let index = match obstacles.iter().position(|(s, _)| *s == schematic) {
                Some(i) => i,
                None => {
                    let rects = schematic_obstacles(tx.project(), schematic, &exclude)?;
                    obstacles.push((schematic, rects));
                    obstacles.len() - 1
                }
            };
            let p = tx.project();
            let sym = p
                .schematic(schematic)
                .and_then(|s| s.symbols().get(&symbol))
                .ok_or_else(|| Error::not_found("Symbol", symbol))?;
            let placed = placed_symbol_rect(p, schematic, symbol, true)?;
            let local = placed.translated(Point::ORIGIN - sym.position());
            let origin = free_position(local, &obstacles[index].1, margin);
            obstacles[index].1.push(local.translated(origin));
            tx.run(MoveSymbol {
                symbol,
                position: Some(origin),
                rotation: None,
                mirrored: None,
            })?;
            positions.push(origin);
        }
        Ok(positions)
    }
}

/// Bounding rectangle of a footprint in footprint coordinates: the
/// courtyard (else package outlines, documentation or legend) united with
/// the pads.
pub fn footprint_rect(footprint: &Footprint) -> Rect {
    let (bl, tr) = footprint.calculate_bounding_rect(true);
    let mut points = Vec::new();
    if bl != tr {
        points.extend([bl, tr]);
    }
    for pad in footprint.pads().iter() {
        let pad = pad.pad();
        let r = (*pad.width()).max(*pad.height()) / 2;
        points.push(Point::new(pad.position().x - r, pad.position().y - r));
        points.push(Point::new(pad.position().x + r, pad.position().y + r));
    }
    Rect::bounding(points).unwrap_or(Rect {
        min: Point::new(Length::new(-500_000), Length::new(-500_000)),
        max: Point::new(Length::new(500_000), Length::new(500_000)),
    })
}

/// Bounding rectangle of a placed device in board coordinates.
pub fn device_rect(p: &Project, device: &BoardDevice) -> Result<Rect> {
    let lib = p.library();
    let dev = lib
        .device(&device.lib_device())
        .ok_or_else(|| Error::not_found("Device", device.lib_device()))?;
    let package = lib
        .package(&dev.package_uuid())
        .ok_or_else(|| Error::not_found("Package", dev.package_uuid()))?;
    let footprint = package
        .footprints()
        .by_uuid(&device.lib_footprint())
        .ok_or_else(|| Error::not_found("Footprint", device.lib_footprint()))?;
    let rect = footprint_rect(footprint).mapped(&device.transform());
    // Stroke texts (name, value), roughly: the anchor with the text height
    // around it and an estimated width.
    let mut points = vec![rect.min, rect.max];
    for text in device.stroke_texts().values() {
        let h = *text.height();
        // Texts are mostly attributes like "{{NAME}}" whose values are
        // short (e.g. "R1"); estimate 0.7 x height per character.
        let chars = text
            .text()
            .chars()
            .filter(|c| *c != '{' && *c != '}')
            .count()
            .max(2) as i64;
        let w = h * chars * 7 / 10;
        let pos = text.position();
        points.push(Point::new(pos.x - w / 2, pos.y - h));
        points.push(Point::new(pos.x + w / 2, pos.y + h));
    }
    Ok(Rect::bounding(points).unwrap_or(rect))
}

/// Returns the outline polygons of a board as closed paths (arcs
/// flattened) and their bounding rectangle, or `None` without outline.
fn board_outline(board: &Board) -> Option<(Vec<Vec<Point>>, Rect)> {
    let tolerance = PositiveLength::new(Length::new(10_000)).ok()?;
    let paths: Vec<Vec<Point>> = board
        .polygons()
        .values()
        .filter(|p| p.layer() == Layer::BOARD_OUTLINES)
        .map(|p| {
            p.path()
                .flattened_arcs(tolerance)
                .vertices()
                .iter()
                .map(|v| v.pos)
                .collect::<Vec<_>>()
        })
        .filter(|v| v.len() >= 3)
        .collect();
    // The outer outline is the one with the largest bounding rect.
    let rect = paths
        .iter()
        .filter_map(|v| Rect::bounding(v.iter().copied()))
        .max_by_key(|r| (r.width().to_nm() as i128) * (r.height().to_nm() as i128))?;
    Some((paths, rect))
}

/// Point in polygon (even-odd ray casting).
fn inside(polygon: &[Point], p: Point) -> bool {
    let mut result = false;
    let n = polygon.len();
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + n - 1) % n];
        if (a.y > p.y) != (b.y > p.y) {
            let (ax, ay) = (a.x.to_nm() as f64, a.y.to_nm() as f64);
            let (bx, by) = (b.x.to_nm() as f64, b.y.to_nm() as f64);
            let x = (bx - ax) * (p.y.to_nm() as f64 - ay) / (by - ay) + ax;
            if (p.x.to_nm() as f64) < x {
                result = !result;
            }
        }
    }
    result
}

/// Adds the devices of components to a board, packed row by row (from the
/// top left) inside the board outline without overlapping other devices,
/// keeping `spacing` between footprints and the board clearance of the
/// DRC settings to the board edge. Components without device on the board
/// are placed (default: all which have an assembly option and are not
/// schematic-only), with the device of their first assembly option and
/// the first footprint. Placed components which do not fit are put to the
/// right of the board and reported.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoPlaceDevices {
    /// The board (default: the primary board).
    #[serde(default)]
    pub board: Option<BoardId>,
    /// The components to place (default: all unplaced ones).
    #[serde(default)]
    pub components: Vec<ComponentRef>,
    /// Space between footprints (default: 1 mm).
    #[serde(default)]
    pub spacing: Option<UnsignedLength>,
}

/// Result of [`AutoPlaceDevices`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AutoPlacedDevices {
    /// The board.
    pub board: Option<BoardId>,
    /// The placed components with their device position, inside the
    /// outline.
    pub placed: Vec<(ComponentInstanceId, Point)>,
    /// Components which did not fit into the outline (placed right of
    /// the board).
    pub outside: Vec<ComponentInstanceId>,
    /// Components skipped because they have no device (schematic-only or
    /// no assembly option).
    pub skipped: Vec<ComponentInstanceId>,
}

impl Command for AutoPlaceDevices {
    type Output = AutoPlacedDevices;

    fn text(&self) -> String {
        tr!("CmdAddDeviceToBoard", "Add device to board")
    }

    fn execute(self, tx: &mut Transaction<'_>) -> Result<AutoPlacedDevices> {
        let p = tx.project();
        let board = resolve::board(p, self.board)?;
        let board_id = board.id();
        let (outlines, outline_rect) = board_outline(board).ok_or_else(|| {
            Error::InvalidArgument("The board has no outline; set the outline first.".to_owned())
        })?;
        let spacing = self.spacing.map_or(Length::new(1_000_000), |s| *s);
        let edge = *board.settings().drc_settings.min_copper_board_clearance() + spacing / 2;

        // Components to place.
        let mut todo: Vec<ComponentInstanceId> = Vec::new();
        let mut skipped = Vec::new();
        if self.components.is_empty() {
            let mut all: Vec<(String, ComponentInstanceId)> = p
                .circuit()
                .component_instances()
                .iter()
                .filter(|(id, _)| board.device(**id).is_none())
                .map(|(id, c)| (c.name().to_string(), *id))
                .collect();
            all.sort_by(|a, b| natural_cmp(&a.0, &b.0));
            for (_, id) in all {
                let c = p.circuit().component_instance(id);
                let schematic_only = c
                    .and_then(|c| p.library().component(&c.lib_component()))
                    .is_some_and(|c| c.schematic_only());
                let has_device = c.is_some_and(|c| !c.assembly_options().is_empty());
                if schematic_only || !has_device {
                    skipped.push(id);
                } else {
                    todo.push(id);
                }
            }
        } else {
            for r in &self.components {
                let id = resolve::component(p, r)?;
                if board.device(id).is_some() {
                    return Err(Error::InvalidArgument(format!(
                        "The component \"{}\" is already placed on the board.",
                        resolve::component_name(p, id)
                    )));
                }
                todo.push(id);
            }
        }
        let mut obstacles: Vec<Rect> = board
            .devices()
            .values()
            .map(|d| device_rect(p, d).map(|r| r.expanded(spacing / 2)))
            .collect::<Result<_>>()?;

        // Add all devices first (at the board center), then sort them by
        // size (largest first) for packing.
        let center = Point::new(
            (outline_rect.min.x + outline_rect.max.x) / 2,
            (outline_rect.min.y + outline_rect.max.y) / 2,
        );
        let mut items = Vec::new();
        for id in &todo {
            tx.run(AddDevice {
                component: ComponentRef::Id(*id),
                board: Some(board_id),
                device: None,
                footprint: None,
                position: center,
                rotation: Angle::DEG0,
                mirrored: false,
            })?;
            let p = tx.project();
            let dev = resolve::board(p, Some(board_id))?
                .device(*id)
                .ok_or_else(|| Error::not_found("Device", id))?;
            let local = device_rect(p, dev)?.translated(Point::ORIGIN - center);
            items.push((*id, local));
        }
        items.sort_by_key(|(_, r)| {
            std::cmp::Reverse((r.width().to_nm() as i128) * (r.height().to_nm() as i128))
        });

        let area = outline_rect.expanded(-edge);
        let step = Length::new(500_000);
        let grid = Length::new(100_000);
        let fits = |r: &Rect| {
            area.contains(r)
                && r.expanded(edge)
                    .corners()
                    .iter()
                    .all(|c| outlines.iter().filter(|o| inside(o, *c)).count() % 2 == 1)
        };
        let mut result = AutoPlacedDevices {
            board: Some(board_id),
            skipped,
            ..Default::default()
        };
        let had_devices = !obstacles.is_empty();
        let mut outside_x = outline_rect.max.x + Length::new(5_000_000);
        let mut moves: Vec<(ComponentInstanceId, Point, Option<Rect>)> = Vec::new();
        for (id, local) in items {
            let mut found = None;
            let mut y = area.max.y;
            'rows: while y - local.height() >= area.min.y {
                let mut x = area.min.x;
                while x + local.width() <= area.max.x {
                    let origin =
                        Point::new(snap(x - local.min.x, grid), snap(y - local.max.y, grid));
                    let r = local.translated(origin);
                    let padded = r.expanded(spacing / 2);
                    if fits(&r) && !obstacles.iter().any(|o| o.intersects(&padded)) {
                        found = Some((origin, padded, r));
                        break 'rows;
                    }
                    x += step;
                }
                y -= step;
            }
            match found {
                Some((origin, padded, r)) => {
                    obstacles.push(padded);
                    moves.push((id, origin, Some(r)));
                }
                None => {
                    let origin = Point::new(
                        snap(outside_x - local.min.x, grid),
                        snap(outline_rect.max.y - local.max.y, grid),
                    );
                    outside_x += local.width() + spacing;
                    moves.push((id, origin, None));
                }
            }
        }
        // On an empty board, center the packed group in the outline (if it
        // still fits there).
        let inside_rects: Vec<Rect> = moves.iter().filter_map(|(_, _, r)| *r).collect();
        if !had_devices
            && let Some(group) = Rect::bounding(inside_rects.iter().flat_map(|r| [r.min, r.max]))
        {
            let shift = Point::new(
                snap(
                    (area.min.x + area.max.x) / 2 - (group.min.x + group.max.x) / 2,
                    grid,
                ),
                snap(
                    (area.min.y + area.max.y) / 2 - (group.min.y + group.max.y) / 2,
                    grid,
                ),
            );
            if inside_rects.iter().all(|r| fits(&r.translated(shift))) {
                for (_, origin, r) in moves.iter_mut() {
                    if r.is_some() {
                        *origin += shift;
                    }
                }
            }
        }
        for (id, origin, r) in moves {
            if r.is_some() {
                result.placed.push((id, origin));
            } else {
                result.outside.push(id);
            }
            tx.run(MoveDevice {
                component: ComponentRef::Id(id),
                board: Some(board_id),
                position: Some(origin),
                rotation: None,
                mirrored: None,
                locked: None,
            })?;
        }
        Ok(result)
    }
}

/// Natural string order ("R2" < "R10").
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| {
        let digits: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
        let prefix: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
        (
            prefix,
            digits.parse::<u64>().unwrap_or(u64::MAX),
            s.to_owned(),
        )
    };
    key(a).cmp(&key(b))
}
