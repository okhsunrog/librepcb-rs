//! Scene builder of boards: port of the drawing semantics of
//! libs/librepcb/core/project/board/boardpainter.{h,cpp} and of the board
//! graphics items of libs/librepcb/editor/project/board/graphicsitems/
//! (planes, zones and air wires are drawn by the editor only).
//!
//! Canvas layers ([`BoardSceneLayer`]) are the board layers plus, per
//! copper layer, the THT pads, vias and zones on it (drawn in the pads,
//! vias and zones colors but shown and hidden with the copper layer, like
//! upstream's export does when copper layers are enabled), and the holes,
//! hole fills and air wires. The paint order is upstream's graphics export
//! order (bottom side first for the top view, reversed for the bottom view,
//! which is also mirrored). Differences to upstream:
//!
//! - Standalone pads of net segments use the pad's preview geometries
//!   (the board specific pad geometries are not public in core yet).
//! - Planes are drawn with their fragments if the board has computed them
//!   ([`Board::derived()`]), and always with their outline as a hairline.
//! - Pad, via and hole drills are filled with the background color instead
//!   of being cut out.
//! - Texts are stroke texts; the invisible TrueType texts upstream adds to
//!   PDF/SVG exports are not drawn.

use std::collections::{BTreeSet, HashMap};

use librepcb_canvas::kurbo::{Affine, Circle as KCircle, Line, Rect};
use librepcb_canvas::peniko::Color;
use librepcb_canvas::{Item, ItemId, Layer as CanvasLayer, LayerId, Scene, Style, convert};
use librepcb_core::font::{StrokeFont, StrokeTextPathBuilder};
use librepcb_core::geometry::{Hole, PadGeometry, PadHoleList, Path, TraceAnchor, Via, ZoneLayers};
use librepcb_core::library::pkg::Footprint;
use librepcb_core::project::board::{Board, BoardDevice, BoardNetSegment, BoardStrokeTextData};
use librepcb_core::project::{
    BoardId, ComponentInstanceId, NetSegmentId, NetSignalId, PlaneId, Project,
};
use librepcb_core::types::{Layer, Length, Point, PositiveLength, Uuid};
use librepcb_core::utils::transform::Transform;

use crate::attributes;
use crate::colors::ColorScheme;
use crate::error::{Error, Result};
use crate::shapes::{self, Fill};

/// The side a board is viewed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BoardSide {
    /// Top view.
    #[default]
    Top,
    /// Bottom view (mirrored horizontally, bottom layers on top).
    Bottom,
}

/// A canvas layer of a [`BoardScene`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardSceneLayer {
    /// Items on a board layer.
    Board(Layer),
    /// THT pads on a copper layer.
    ThtPads(Layer),
    /// Vias on a copper layer.
    Vias(Layer),
    /// Zones on a copper layer.
    Zones(Layer),
    /// Hole outlines (non-plated holes).
    Holes,
    /// Drills of pads, vias and holes, filled with the background color.
    HoleFills,
    /// Air wires.
    AirWires,
}

impl BoardSceneLayer {
    /// The canvas layer ID.
    pub fn id(self) -> LayerId {
        let copper = |l: Layer| l.copper_number() as u16;
        match self {
            Self::Board(l) => LayerId(
                Layer::all()
                    .iter()
                    .position(|x| *x == l)
                    .unwrap_or_default() as u16,
            ),
            Self::ThtPads(l) => LayerId(200 + copper(l)),
            Self::Vias(l) => LayerId(300 + copper(l)),
            Self::Zones(l) => LayerId(400 + copper(l)),
            Self::Holes => LayerId(500),
            Self::HoleFills => LayerId(501),
            Self::AirWires => LayerId(502),
        }
    }

    /// The color role (`None` for the hole fills, which use the
    /// background).
    fn color_role(self) -> Option<&'static str> {
        match self {
            Self::Board(l) => Some(l.color_role()),
            Self::ThtPads(_) => Some("board_pads"),
            Self::Vias(_) => Some("board_vias"),
            Self::Zones(_) => Some("board_zones"),
            Self::Holes => Some("board_holes"),
            Self::HoleFills => None,
            Self::AirWires => Some("board_airwires"),
        }
    }

    /// The board layer whose visibility the layer follows.
    fn board_layer(self) -> Option<Layer> {
        match self {
            Self::Board(l) | Self::ThtPads(l) | Self::Vias(l) | Self::Zones(l) => Some(l),
            _ => None,
        }
    }
}

/// The model object an item of a [`BoardScene`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardObject {
    /// A device (footprint shapes, holes, zones and texts).
    Device(ComponentInstanceId),
    /// A footprint pad of a device.
    FootprintPad(ComponentInstanceId, Uuid),
    /// A standalone pad of a net segment.
    Pad(NetSegmentId, Uuid),
    /// A via of a net segment.
    Via(NetSegmentId, Uuid),
    /// A trace of a net segment.
    Trace(NetSegmentId, Uuid),
    /// A plane.
    Plane(PlaneId),
    /// A zone of the board.
    Zone(Uuid),
    /// A polygon of the board.
    Polygon(Uuid),
    /// A stroke text of the board or of a device.
    StrokeText(Uuid),
    /// A hole of the board.
    Hole(Uuid),
    /// An air wire of a net.
    AirWire(NetSignalId),
}

/// Sub-ordering of items within one canvas layer.
mod sub {
    pub const AREAS: i32 = 0;
    pub const PADS: i32 = 1;
    pub const LINES: i32 = 2;
    pub const TEXTS: i32 = 3;
}

/// A board as a canvas [`Scene`], seen from one side.
#[derive(Debug)]
pub struct BoardScene {
    scene: Scene,
    scheme: ColorScheme,
    side: BoardSide,
    objects: HashMap<ItemId, BoardObject>,
    layers: Vec<BoardSceneLayer>,
    warnings: Vec<String>,
}

static_assertions::assert_impl_all!(BoardScene: Send, Sync);

impl BoardScene {
    /// Builds the scene of a board seen from `side` with a color scheme
    /// (e.g. [`ColorScheme::BOARD_DARK`]). The layer visibility is taken
    /// from the board's user settings (all layers are visible by default).
    /// Items which cannot be resolved are skipped and reported by
    /// [`warnings()`](Self::warnings).
    pub fn build(
        project: &Project,
        board: BoardId,
        side: BoardSide,
        scheme: &ColorScheme,
    ) -> Result<Self> {
        let b = project.board(board).ok_or(Error::BoardNotFound(board))?;
        let copper_layers = b.copper_layers();
        let font = project.stroke_fonts().font(&b.settings().default_font).ok();
        let mut builder = Builder {
            project,
            board: b,
            scene: Scene::new(),
            scheme: *scheme,
            objects: HashMap::new(),
            ranks: HashMap::new(),
            warnings: Vec::new(),
            copper_layers,
            font,
            pad_positions: HashMap::new(),
        };
        let layers = paint_order(&builder.copper_layers, side);
        builder.add_layers(&layers);
        builder.add_devices();
        builder.add_net_segments();
        builder.add_planes();
        builder.add_board_items();
        builder.add_air_wires();
        let mut scene = Self {
            scene: builder.scene,
            scheme: *scheme,
            side,
            objects: builder.objects,
            layers,
            warnings: builder.warnings,
        };
        for (id, visible) in b.layers_visibility() {
            if let Some(layer) = Layer::from_id(id) {
                scene.set_layer_visible(layer, *visible);
            }
        }
        Ok(scene)
    }

    /// The side the board is viewed from.
    pub fn side(&self) -> BoardSide {
        self.side
    }

    /// The scene.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The scene for modification (e.g. selection).
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// The model object of an item.
    pub fn object(&self, item: ItemId) -> Option<BoardObject> {
        self.objects.get(&item).copied()
    }

    /// All canvas layers in paint order (first is painted first).
    pub fn layers(&self) -> &[BoardSceneLayer] {
        &self.layers
    }

    /// Shows or hides a board layer together with the THT pads, vias and
    /// zones on it.
    pub fn set_layer_visible(&mut self, layer: Layer, visible: bool) {
        let ids: Vec<LayerId> = self
            .layers
            .iter()
            .filter(|l| l.board_layer() == Some(layer))
            .map(|l| l.id())
            .collect();
        for id in ids {
            self.scene.set_layer_visible(id, visible);
        }
    }

    /// Shows or hides one canvas layer.
    pub fn set_scene_layer_visible(&mut self, layer: BoardSceneLayer, visible: bool) {
        self.scene.set_layer_visible(layer.id(), visible);
    }

    /// The background color of the color scheme.
    pub fn background(&self) -> Color {
        self.scheme.color_or_transparent("board_background")
    }

    /// Changes the color of the hole fills (should be the background).
    pub fn set_background(&mut self, color: Color) {
        self.scene
            .set_layer_color(BoardSceneLayer::HoleFills.id(), color);
    }

    /// Problems found while building (skipped items).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Bounding box of all visible items (world coordinates, mm).
    pub fn content_bounds(&self) -> Option<Rect> {
        crate::render::visible_bounds(&self.scene)
    }
}

/// The canvas layers in paint order: upstream's graphics export order
/// (`GraphicsExportSettings::loadColorsFromScheme()`, painted in reverse),
/// with the package outlines and finish layers added next to their
/// neighbors.
fn paint_order(copper_layers: &BTreeSet<Layer>, side: BoardSide) -> Vec<BoardSceneLayer> {
    use BoardSceneLayer as S;
    let copper = |l: Layer| [S::Board(l), S::Zones(l), S::ThtPads(l), S::Vias(l)];
    let mut symmetric = vec![
        S::Board(Layer::BOT_DOCUMENTATION),
        S::Board(Layer::BOT_NAMES),
        S::Board(Layer::BOT_VALUES),
        S::Board(Layer::BOT_COURTYARD),
        S::Board(Layer::BOT_PACKAGE_OUTLINES),
        S::Board(Layer::BOT_LEGEND),
        S::Board(Layer::BOT_GLUE),
        S::Board(Layer::BOT_FINISH),
        S::Board(Layer::BOT_SOLDER_PASTE),
        S::Board(Layer::BOT_STOP_MASK),
    ];
    symmetric.extend(copper(Layer::BOT_COPPER));
    for layer in copper_layers.iter().rev().filter(|l| l.is_inner()) {
        symmetric.extend(copper(*layer));
    }
    symmetric.extend(copper(Layer::TOP_COPPER));
    symmetric.extend([
        S::Board(Layer::TOP_STOP_MASK),
        S::Board(Layer::TOP_SOLDER_PASTE),
        S::Board(Layer::TOP_FINISH),
        S::Board(Layer::TOP_GLUE),
        S::Board(Layer::TOP_LEGEND),
        S::Board(Layer::TOP_PACKAGE_OUTLINES),
        S::Board(Layer::TOP_COURTYARD),
        S::Board(Layer::TOP_VALUES),
        S::Board(Layer::TOP_NAMES),
        S::Board(Layer::TOP_DOCUMENTATION),
    ]);
    if side == BoardSide::Bottom {
        // Keep each copper group's internal order.
        let mut groups: Vec<Vec<S>> = Vec::new();
        for l in symmetric {
            match (l, groups.last_mut()) {
                (S::Zones(_) | S::ThtPads(_) | S::Vias(_), Some(g)) => g.push(l),
                _ => groups.push(vec![l]),
            }
        }
        symmetric = groups.into_iter().rev().flatten().collect();
    }
    symmetric.extend([
        S::HoleFills,
        S::Board(Layer::BOARD_PLATED_CUTOUTS),
        S::Holes,
        S::Board(Layer::BOARD_CUTOUTS),
        S::Board(Layer::BOARD_OUTLINES),
        S::AirWires,
        S::Board(Layer::BOARD_SHEET_FRAMES),
        S::Board(Layer::BOARD_MEASURES),
        S::Board(Layer::BOARD_ALIGNMENT),
        S::Board(Layer::BOARD_DOCUMENTATION),
        S::Board(Layer::BOARD_COMMENTS),
        S::Board(Layer::BOARD_GUIDE),
    ]);
    symmetric
}

struct Builder<'a> {
    project: &'a Project,
    board: &'a Board,
    scene: Scene,
    scheme: ColorScheme,
    objects: HashMap<ItemId, BoardObject>,
    ranks: HashMap<LayerId, i32>,
    warnings: Vec<String>,
    copper_layers: BTreeSet<Layer>,
    font: Option<&'a StrokeFont>,
    /// Positions of footprint pads (device, pad).
    pad_positions: HashMap<(Uuid, Uuid), Point>,
}

impl Builder<'_> {
    fn add_layers(&mut self, layers: &[BoardSceneLayer]) {
        let background = self.scheme.color_or_transparent("board_background");
        for (rank, layer) in layers.iter().enumerate() {
            let rank = rank as i32;
            let color = layer
                .color_role()
                .map_or(background, |r| self.scheme.color_or_transparent(r));
            self.scene
                .set_layer(layer.id(), CanvasLayer::new(color).with_order(rank));
            self.ranks.insert(layer.id(), rank);
        }
    }

    /// Inserts an item on `layer` (skipped if the layer is not drawn, e.g.
    /// hidden grab areas or disabled inner layers).
    fn insert(
        &mut self,
        layer: BoardSceneLayer,
        sub: i32,
        item: Option<Item>,
        object: BoardObject,
    ) {
        let id = layer.id();
        let Some(rank) = self.ranks.get(&id).copied() else {
            return;
        };
        if let Some(mut item) = item
            && !item.geometry.is_empty()
        {
            item.layer = id;
            item.z = rank * 4 + sub;
            let item_id = self.scene.insert(item);
            self.objects.insert(item_id, object);
        }
    }

    fn grab_area_fill(&self, layer: Layer) -> Fill {
        // Upstream `ColorRole::getGrabAreaRole()`.
        let role = if layer == Layer::TOP_LEGEND {
            "board_grab_areas_top"
        } else if layer == Layer::BOT_LEGEND {
            "board_grab_areas_bottom"
        } else {
            return Fill::None;
        };
        self.scheme.color(role).map_or(Fill::None, Fill::Solid)
    }

    fn polygon_fill(&self, layer: Layer, filled: bool, grab_area: bool) -> Fill {
        if filled {
            Fill::Layer
        } else if grab_area {
            self.grab_area_fill(layer)
        } else {
            Fill::None
        }
    }

    fn stroke_text(&mut self, data: &BoardStrokeTextData, text: &str, object: BoardObject) {
        let Some(font) = self.font else {
            return;
        };
        let paths = StrokeTextPathBuilder::build(
            font,
            data.letter_spacing(),
            data.line_spacing(),
            data.height(),
            data.stroke_width(),
            data.align(),
            data.rotation(),
            data.auto_rotate(),
            text,
        );
        let xf = convert::transform(&Transform::new(
            data.position(),
            data.rotation(),
            data.mirrored(),
        ));
        let item = Item::new(
            LayerId::default(),
            xf * convert::paths(&paths),
            Style::stroke(shapes::pen(*data.stroke_width())),
        );
        self.insert(
            BoardSceneLayer::Board(data.layer()),
            sub::TEXTS,
            Some(item),
            object,
        );
    }

    /// A non-plated hole: outline, drill fill and stop mask openings.
    fn hole(
        &mut self,
        diameter: PositiveLength,
        path: &Path,
        stop_mask: Option<Length>,
        object: BoardObject,
    ) {
        let strokes = path.to_outline_strokes(diameter);
        let outline = shapes::outline(LayerId::default(), &strokes, Affine::IDENTITY);
        self.insert(BoardSceneLayer::Holes, sub::LINES, outline, object);
        let fill = shapes::area(LayerId::default(), &strokes, Affine::IDENTITY);
        self.insert(BoardSceneLayer::HoleFills, sub::AREAS, fill, object);
        if let Some(offset) = stop_mask
            && let Ok(path) = librepcb_core::geometry::NonEmptyPath::new(path.clone())
        {
            let geometry =
                PadGeometry::stroke(diameter, path, PadHoleList::new()).with_offset(offset);
            if let Ok(outlines) = geometry.to_outlines() {
                for layer in [Layer::TOP_STOP_MASK, Layer::BOT_STOP_MASK] {
                    let area = shapes::area(LayerId::default(), &outlines, Affine::IDENTITY);
                    self.insert(BoardSceneLayer::Board(layer), sub::AREAS, area, object);
                }
            }
        }
    }

    fn zone(
        &mut self,
        layers: impl IntoIterator<Item = Layer>,
        outline: &Path,
        xf: Affine,
        object: BoardObject,
    ) {
        let mut path = outline.clone();
        path.close();
        for layer in layers {
            if !self.copper_layers.contains(&layer) {
                continue;
            }
            let item = shapes::polygon(LayerId::default(), &path, xf, Length::ZERO, Fill::Layer)
                .map(|mut i| {
                    i.style.stroke = Some(librepcb_canvas::StrokeStyle::hairline());
                    i
                });
            self.insert(BoardSceneLayer::Zones(layer), sub::AREAS, item, object);
        }
    }

    fn add_devices(&mut self) {
        let devices: Vec<&BoardDevice> = self.board.devices().values().collect();
        for device in devices {
            if let Err(e) = self.add_device(device) {
                self.warnings
                    .push(format!("Device {}: {e}", device.component().0));
            }
        }
    }

    fn resolve_footprint<'b>(&self, device: &BoardDevice) -> Result<&'b Footprint>
    where
        Self: 'b,
    {
        use librepcb_core::project::Error as PErr;
        let lib = self.project.library();
        let lib_device = lib
            .device(&device.lib_device())
            .ok_or(PErr::MissingLibraryDevice(device.lib_device()))?;
        let package = lib
            .package(&lib_device.package_uuid())
            .ok_or(PErr::MissingLibraryPackage(lib_device.package_uuid()))?;
        let footprint = package
            .footprints()
            .by_uuid(&device.lib_footprint())
            .ok_or(PErr::MissingLibraryPackage(device.lib_footprint()))?;
        Ok(footprint)
    }

    fn add_device(&mut self, device: &BoardDevice) -> Result<()> {
        let project = self.project;
        let board = self.board;
        let footprint = self.resolve_footprint(device)?;
        let transform = device.transform();
        let xf = convert::transform(&transform);
        let object = BoardObject::Device(device.component());
        let rules = board.design_rules();

        for polygon in footprint.polygons().iter() {
            let layer = transform.map(&polygon.layer());
            let fill = self.polygon_fill(layer, polygon.is_filled(), polygon.is_grab_area());
            let item = shapes::polygon(
                LayerId::default(),
                polygon.path(),
                xf,
                *polygon.line_width(),
                fill,
            );
            self.insert(BoardSceneLayer::Board(layer), sub::LINES, item, object);
        }
        for circle in footprint.circles().iter() {
            let layer = transform.map(&circle.layer());
            let fill = self.polygon_fill(layer, circle.is_filled(), circle.is_grab_area());
            let item = shapes::circle(
                LayerId::default(),
                xf * convert::point(circle.center()),
                *circle.diameter(),
                *circle.line_width(),
                fill,
            );
            self.insert(
                BoardSceneLayer::Board(layer),
                sub::LINES,
                Some(item),
                object,
            );
        }
        let stop_masks = BoardDevice::hole_stop_mask_offsets(footprint, rules);
        for hole in footprint.holes().iter() {
            let hole: &Hole = hole;
            let path = transform.map(hole.path().get());
            let offset = stop_masks.get(&hole.uuid()).copied().flatten();
            self.hole(hole.diameter(), &path, offset, object);
        }
        for zone in footprint.zones().iter() {
            let flags = zone.layers();
            let (top, bottom) = if transform.mirrored {
                (ZoneLayers::BOTTOM, ZoneLayers::TOP)
            } else {
                (ZoneLayers::TOP, ZoneLayers::BOTTOM)
            };
            let mut layers = Vec::new();
            if flags.contains(top) {
                layers.push(Layer::TOP_COPPER);
            }
            if flags.contains(ZoneLayers::INNER) {
                layers.extend(self.copper_layers.iter().filter(|l| l.is_inner()));
            }
            if flags.contains(bottom) {
                layers.push(Layer::BOT_COPPER);
            }
            self.zone(layers, zone.outline(), xf, object);
        }

        // Pads.
        let pads = device.pads(project.library(), project.circuit())?;
        for pad in &pads {
            let object = BoardObject::FootprintPad(device.component(), pad.uuid());
            self.pad_positions
                .insert((device.component().0, pad.uuid()), pad.position());
            let pad_xf = convert::transform(&Transform::new(
                pad.position(),
                pad.rotation(),
                pad.mirrored(),
            ));
            let connected = board.anchor_trace_layers(pad.trace_anchor());
            let holes = pad.properties().holes();
            let geometries = pad.geometries(&self.copper_layers, rules, &connected);
            self.pad_geometries(&geometries, holes, pad_xf, object);
        }

        // Texts.
        for text in device.stroke_texts().values() {
            let value = attributes::substitute_device_text(project, board, device, text.text());
            self.stroke_text(text, &value, BoardObject::StrokeText(text.uuid()));
        }
        Ok(())
    }

    fn pad_geometries(
        &mut self,
        geometries: &std::collections::BTreeMap<Layer, Vec<PadGeometry>>,
        holes: &PadHoleList,
        xf: Affine,
        object: BoardObject,
    ) {
        for (layer, geometries) in geometries {
            let scene_layer = if !holes.is_empty() && layer.is_copper() {
                BoardSceneLayer::ThtPads(*layer)
            } else {
                BoardSceneLayer::Board(*layer)
            };
            for geometry in geometries {
                match geometry.to_outlines() {
                    Ok(outlines) => {
                        let item = shapes::area(LayerId::default(), &outlines, xf);
                        self.insert(scene_layer, sub::PADS, item, object);
                    }
                    Err(e) => self.warnings.push(format!("Pad geometry: {e}")),
                }
            }
        }
        for hole in holes.iter() {
            let strokes = hole.path().get().to_outline_strokes(hole.diameter());
            let fill = shapes::area(LayerId::default(), &strokes, xf);
            self.insert(BoardSceneLayer::HoleFills, sub::AREAS, fill, object);
        }
    }

    fn trace_anchor_position(
        &self,
        segment: &BoardNetSegment,
        anchor: TraceAnchor,
    ) -> Option<Point> {
        match anchor {
            TraceAnchor::Junction(uuid) => segment.junctions().get(&uuid).map(|j| j.position()),
            TraceAnchor::Via(uuid) => segment.vias().get(&uuid).map(|v| v.position()),
            TraceAnchor::Pad(uuid) => segment.pads().get(&uuid).map(|p| p.pad().position()),
            TraceAnchor::FootprintPad { device, pad } => {
                self.pad_positions.get(&(device, pad)).copied()
            }
        }
    }

    fn add_net_segments(&mut self) {
        let board = self.board;
        let rules = board.design_rules();
        for (id, segment) in board.net_segments() {
            let id = *id;
            // Standalone pads.
            for (uuid, pad) in segment.pads() {
                let p = pad.pad();
                let xf = convert::transform(&Transform::new(p.position(), p.rotation(), false));
                // TODO: use the board pad geometries (`BI_Pad::getGeometries()`)
                // once core exposes them for standalone pads.
                let geometries = p.build_preview_geometries();
                self.pad_geometries(&geometries, p.holes(), xf, BoardObject::Pad(id, *uuid));
            }
            // Vias.
            for (uuid, via) in segment.vias() {
                let object = BoardObject::Via(id, *uuid);
                let (drill, size) = self.via_drill_and_size(segment, via);
                let center = convert::point(via.position());
                for layer in self.copper_layers.clone() {
                    if Via::is_on_layer_between(layer, via.start_layer(), via.end_layer()) {
                        let item = Item::new(
                            LayerId::default(),
                            KCircle::new(center, size.to_mm() / 2.0),
                            Style::fill(),
                        );
                        self.insert(BoardSceneLayer::Vias(layer), sub::PADS, Some(item), object);
                    }
                }
                let fill = Item::new(
                    LayerId::default(),
                    KCircle::new(center, drill.to_mm() / 2.0),
                    Style::fill(),
                );
                self.insert(BoardSceneLayer::HoleFills, sub::AREAS, Some(fill), object);
                // Stop mask openings (upstream `BI_Via::updateStopMaskDiameters()`).
                let config = via.exposure_config();
                let dia = if let Some(offset) = config.offset() {
                    *size + offset * 2
                } else if config.is_enabled() {
                    *size + *rules.stop_mask_clearance().calc_value(*size) * 2
                } else if rules.does_via_require_stop_mask_opening(*drill) {
                    *drill + *rules.stop_mask_clearance().calc_value(*drill) * 2
                } else {
                    Length::ZERO
                };
                if dia > Length::ZERO {
                    let circle = KCircle::new(center, dia.to_mm() / 2.0);
                    if via.start_layer().is_top() {
                        let item = Item::new(LayerId::default(), circle, Style::fill());
                        self.insert(
                            BoardSceneLayer::Board(Layer::TOP_STOP_MASK),
                            sub::AREAS,
                            Some(item),
                            object,
                        );
                    }
                    if via.end_layer().is_bottom() {
                        let item = Item::new(LayerId::default(), circle, Style::fill());
                        self.insert(
                            BoardSceneLayer::Board(Layer::BOT_STOP_MASK),
                            sub::AREAS,
                            Some(item),
                            object,
                        );
                    }
                }
            }
            // Traces.
            for (uuid, trace) in segment.traces() {
                match (
                    self.trace_anchor_position(segment, trace.p1()),
                    self.trace_anchor_position(segment, trace.p2()),
                ) {
                    (Some(p1), Some(p2)) => {
                        let item = shapes::line(LayerId::default(), p1, p2, *trace.width());
                        self.insert(
                            BoardSceneLayer::Board(trace.layer()),
                            sub::LINES,
                            Some(item),
                            BoardObject::Trace(id, *uuid),
                        );
                    }
                    _ => self
                        .warnings
                        .push(format!("Trace {uuid}: unresolved anchor")),
                }
            }
        }
    }

    /// Upstream `BI_Via::updateActualDrillAndSize()`.
    fn via_drill_and_size(
        &self,
        segment: &BoardNetSegment,
        via: &Via,
    ) -> (PositiveLength, PositiveLength) {
        let rules = self.board.design_rules();
        let circuit = self.project.circuit();
        let drill = via.drill_diameter().unwrap_or_else(|| {
            segment
                .net()
                .and_then(|n| circuit.net_signal(n))
                .and_then(|n| circuit.net_class(n.net_class()))
                .and_then(|c| c.default_via_drill())
                .unwrap_or(rules.default_via_drill_diameter())
        });
        let size = match via.size() {
            Some(size) => size.max(drill),
            None => Via::calc_size_from_rules(drill, &rules.via_annular_ring()),
        };
        (drill, size)
    }

    fn add_planes(&mut self) {
        let board = self.board;
        for (id, plane) in board.planes() {
            let object = BoardObject::Plane(*id);
            let layer = BoardSceneLayer::Board(plane.layer());
            let fragments = board.derived().fragments_of(*id);
            if plane.visible() && !fragments.is_empty() {
                let item = shapes::area(LayerId::default(), fragments, Affine::IDENTITY);
                self.insert(layer, sub::AREAS, item, object);
            }
            let mut outline = plane.outline().clone();
            outline.close();
            let item = shapes::outline(LayerId::default(), &[outline], Affine::IDENTITY);
            self.insert(layer, sub::AREAS, item, object);
        }
    }

    fn add_board_items(&mut self) {
        let project = self.project;
        let board = self.board;
        for (uuid, zone) in board.zones() {
            self.zone(
                zone.layers().iter().copied(),
                zone.outline(),
                Affine::IDENTITY,
                BoardObject::Zone(*uuid),
            );
        }
        for (uuid, polygon) in board.polygons() {
            let fill =
                self.polygon_fill(polygon.layer(), polygon.is_filled(), polygon.is_grab_area());
            let item = shapes::polygon(
                LayerId::default(),
                polygon.path(),
                Affine::IDENTITY,
                *polygon.line_width(),
                fill,
            );
            self.insert(
                BoardSceneLayer::Board(polygon.layer()),
                sub::LINES,
                item,
                BoardObject::Polygon(*uuid),
            );
        }
        for (uuid, text) in board.stroke_texts() {
            let value = attributes::substitute_board_text(project, board, text.text());
            self.stroke_text(text, &value, BoardObject::StrokeText(*uuid));
        }
        let rules = board.design_rules();
        for (uuid, hole) in board.holes() {
            self.hole(
                hole.diameter(),
                hole.path().get(),
                hole.stop_mask_offset(rules),
                BoardObject::Hole(*uuid),
            );
        }
    }

    fn add_air_wires(&mut self) {
        let wires: Vec<_> = self.board.derived().all_air_wires().cloned().collect();
        for wire in wires {
            let item = Item::new(
                LayerId::default(),
                Line::new(
                    convert::point(wire.p1_position()),
                    convert::point(wire.p2_position()),
                ),
                Style::stroke(0.0),
            );
            self.insert(
                BoardSceneLayer::AirWires,
                sub::LINES,
                Some(item),
                BoardObject::AirWire(wire.net()),
            );
        }
    }
}
