//! Scene builder of schematic pages: port of the drawing semantics of
//! libs/librepcb/core/project/schematic/schematicpainter.{h,cpp} (which the
//! upstream graphics export uses) and of the schematic graphics items of
//! libs/librepcb/editor/project/schematic/graphicsitems/.
//!
//! Every color role of the upstream graphics export is a canvas layer
//! ([`SchematicScene::layer_id()`]); items are ordered like the painter
//! draws them (grab areas, filled shapes, outlines and pins of symbols,
//! then polygons, texts, net lines, junctions, net labels and the bus
//! items). Differences to upstream:
//!
//! - Texts are drawn with the project's stroke font (see [`crate::text`]).
//! - Images are not drawn yet, only their borders.
//! - Pins with a forced net name conflict (`hasError()`, not ported in core)
//!   are drawn with their full length.

use std::collections::{BTreeSet, HashMap};

use librepcb_canvas::kurbo::{Affine, Circle as KCircle, Rect, Shape};
use librepcb_canvas::peniko::Color;
use librepcb_canvas::{Item, ItemId, Layer as CanvasLayer, LayerId, Scene, Style, convert};
use librepcb_core::geometry::{NetLineAnchor, Text};
use librepcb_core::library::sym::SymbolPin;
use librepcb_core::project::schematic::{Schematic, SchematicSymbol, SymbolPinView};
use librepcb_core::project::{BusSegmentId, NetSegmentId, Project, SchematicId, SymbolId};
use librepcb_core::types::{Length, Point, Uuid};
use librepcb_core::utils::toolbox;

use crate::attributes;
use crate::colors::ColorScheme;
use crate::error::{Error, Result};
use crate::shapes::{self, Fill};
use crate::text::{RenderedText, TextRenderer, TextStyle};

/// Color roles of the schematic in upstream's graphics export order
/// (`GraphicsExportSettings::loadColorsFromScheme()`); painted in reverse
/// order, so the first role is on top.
const ROLES: &[&str] = &[
    "schematic_frames",
    "schematic_outlines",
    "schematic_grab_areas",
    "schematic_pin_lines",
    "schematic_pin_names",
    "schematic_pin_numbers",
    "schematic_names",
    "schematic_values",
    "schematic_wires",
    "schematic_net_labels",
    "schematic_buses",
    "schematic_bus_labels",
    "schematic_image_borders",
    "schematic_documentation",
    "schematic_comments",
    "schematic_guide",
];

/// Draw order of the item kinds (upstream `SchematicPainter::paint()`).
mod z {
    pub const GRAB_AREAS: i32 = 0;
    pub const FILLED: i32 = 1;
    pub const SYMBOL_IMAGES: i32 = 2;
    pub const SHAPES: i32 = 3;
    pub const PINS: i32 = 4;
    pub const PIN_TEXTS: i32 = 5;
    pub const IMAGES: i32 = 6;
    pub const POLYGONS: i32 = 7;
    pub const TEXTS: i32 = 8;
    pub const NET_LINES: i32 = 9;
    pub const NET_JUNCTIONS: i32 = 10;
    pub const NET_LABELS: i32 = 11;
    pub const BUS_LINES: i32 = 12;
    pub const BUS_JUNCTIONS: i32 = 13;
    pub const BUS_LABELS: i32 = 14;
}

/// Radius of junction dots (upstream `drawNetJunction()`).
const JUNCTION_RADIUS: f64 = 0.6;
/// Line width of symbol pins (upstream `drawSymbolPin()`).
const PIN_LINE_WIDTH: Length = Length::new(158_750);

/// The model object an item of a [`SchematicScene`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchematicObject {
    /// A symbol (its shapes and texts).
    Symbol(SymbolId),
    /// A pin of a symbol (library pin UUID).
    SymbolPin(SymbolId, Uuid),
    /// A net line of a net segment.
    NetLine(NetSegmentId, Uuid),
    /// A junction dot (net segment junction or symbol pin).
    NetJunction(NetSegmentId, NetLineAnchor),
    /// A net label of a net segment.
    NetLabel(NetSegmentId, Uuid),
    /// A line of a bus segment.
    BusLine(BusSegmentId, Uuid),
    /// A junction of a bus segment.
    BusJunction(BusSegmentId, Uuid),
    /// A label of a bus segment.
    BusLabel(BusSegmentId, Uuid),
    /// A polygon of the schematic.
    Polygon(Uuid),
    /// A text of the schematic.
    Text(Uuid),
    /// An image of the schematic.
    Image(Uuid),
}

/// A schematic page as a canvas [`Scene`].
#[derive(Debug)]
pub struct SchematicScene {
    scene: Scene,
    scheme: ColorScheme,
    objects: HashMap<ItemId, SchematicObject>,
    warnings: Vec<String>,
}

static_assertions::assert_impl_all!(SchematicScene: Send, Sync);

impl SchematicScene {
    /// Builds the scene of a schematic page with a color scheme (e.g.
    /// [`ColorScheme::SCHEMATIC_LIGHT`]). Items which cannot be resolved
    /// (e.g. symbols missing in the project library) are skipped and
    /// reported by [`warnings()`](Self::warnings).
    pub fn build(project: &Project, schematic: SchematicId, scheme: &ColorScheme) -> Result<Self> {
        let sch = project
            .schematic(schematic)
            .ok_or(Error::SchematicNotFound(schematic))?;
        let mut builder = Builder {
            project,
            schematic: sch,
            scene: Scene::new(),
            scheme: *scheme,
            objects: HashMap::new(),
            warnings: Vec::new(),
            text: TextRenderer::new(schematic_font(project)),
            pin_positions: HashMap::new(),
            pad_names: HashMap::new(),
        };
        builder.add_layers();
        builder.add_symbols();
        builder.add_page_items();
        builder.add_net_segments();
        builder.add_bus_segments();
        Ok(Self {
            scene: builder.scene,
            scheme: *scheme,
            objects: builder.objects,
            warnings: builder.warnings,
        })
    }

    /// The canvas layer of a color role (e.g. `"schematic_wires"`), `None`
    /// if the role is not drawn.
    pub fn layer_id(role: &str) -> Option<LayerId> {
        ROLES
            .iter()
            .position(|r| *r == role)
            .map(|i| LayerId(i as u16))
    }

    /// The scene.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The scene for modification (e.g. layer visibility, selection).
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// The model object of an item.
    pub fn object(&self, item: ItemId) -> Option<SchematicObject> {
        self.objects.get(&item).copied()
    }

    /// The background color of the color scheme.
    pub fn background(&self) -> Color {
        self.scheme.color_or_transparent("schematic_background")
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

/// The stroke font used for the texts: the default board font (the
/// schematic has no stroke font setting), or any font of the project.
fn schematic_font(project: &Project) -> Option<&librepcb_core::font::StrokeFont> {
    let pool = project.stroke_fonts();
    pool.font(librepcb_core::project::board::DEFAULT_STROKE_FONT_NAME)
        .ok()
}

struct Builder<'a> {
    project: &'a Project,
    schematic: &'a Schematic,
    scene: Scene,
    scheme: ColorScheme,
    objects: HashMap<ItemId, SchematicObject>,
    warnings: Vec<String>,
    text: TextRenderer<'a>,
    /// Absolute positions of all connected symbol pins.
    pin_positions: HashMap<(Uuid, Uuid), Point>,
    /// Pad names per component signal (component, signal).
    pad_names: HashMap<(Uuid, Uuid), Vec<String>>,
}

impl Builder<'_> {
    fn add_layers(&mut self) {
        let count = ROLES.len() as i32;
        for (i, role) in ROLES.iter().enumerate() {
            let color = self.scheme.color_or_transparent(role);
            self.scene.set_layer(
                LayerId(i as u16),
                CanvasLayer::new(color).with_order(count - i as i32),
            );
        }
    }

    fn insert(&mut self, item: Option<Item>, object: SchematicObject) {
        if let Some(item) = item
            && !item.geometry.is_empty()
        {
            let id = self.scene.insert(item);
            self.objects.insert(id, object);
        }
    }

    fn insert_text(
        &mut self,
        layer: LayerId,
        z: i32,
        rendered: Option<RenderedText>,
        object: SchematicObject,
    ) {
        let item =
            rendered.map(|t| Item::new(layer, t.path, Style::stroke(t.stroke_width)).with_z(z));
        self.insert(item, object);
    }

    /// Canvas layer of a model layer, `None` if the role is not drawn.
    fn layer(&self, layer: librepcb_core::types::Layer) -> Option<LayerId> {
        SchematicScene::layer_id(layer.color_role())
    }

    fn grab_area_fill(&self, role_layer: librepcb_core::types::Layer) -> Fill {
        // Upstream `ColorRole::getGrabAreaRole()`.
        if role_layer.color_role() == "schematic_outlines" {
            self.scheme
                .color("schematic_grab_areas")
                .map_or(Fill::None, Fill::Solid)
        } else {
            Fill::None
        }
    }

    fn add_symbols(&mut self) {
        let symbols: Vec<&SchematicSymbol> = self.schematic.symbols().values().collect();
        for symbol in symbols {
            if let Err(e) = self.add_symbol(symbol) {
                self.warnings.push(format!("Symbol {}: {e}", symbol.uuid()));
            }
        }
    }

    fn add_symbol(&mut self, symbol: &SchematicSymbol) -> Result<()> {
        let view = self.project.view();
        let resolved = symbol.resolve(view)?;
        let pins = symbol.pins(view)?;
        let lib = resolved.lib_symbol;
        let transform = symbol.transform();
        let xf = convert::transform(&transform);
        let object = SchematicObject::Symbol(symbol.id());

        // Shapes (upstream `drawShapes()` for grab areas, filled shapes and
        // the others).
        for polygon in lib.polygons().iter() {
            let Some(layer) = self.layer(polygon.layer()) else {
                continue;
            };
            let (z, fill) = if polygon.is_filled() {
                (z::FILLED, Fill::Layer)
            } else if polygon.is_grab_area() {
                (z::GRAB_AREAS, self.grab_area_fill(polygon.layer()))
            } else {
                (z::SHAPES, Fill::None)
            };
            let item = shapes::polygon(layer, polygon.path(), xf, *polygon.line_width(), fill)
                .map(|i| i.with_z(z));
            self.insert(item, object);
        }
        for circle in lib.circles().iter() {
            let Some(layer) = self.layer(circle.layer()) else {
                continue;
            };
            let (z, fill) = if circle.is_filled() {
                (z::FILLED, Fill::Layer)
            } else if circle.is_grab_area() {
                (z::GRAB_AREAS, self.grab_area_fill(circle.layer()))
            } else {
                (z::SHAPES, Fill::None)
            };
            let center = convert::point(transform.map(&circle.center()));
            let item = shapes::circle(
                layer,
                center,
                *circle.diameter(),
                *circle.line_width(),
                fill,
            )
            .with_z(z);
            self.insert(Some(item), object);
        }
        // Image borders (the images themselves are not drawn yet).
        if let Some(layer) = SchematicScene::layer_id("schematic_image_borders") {
            for image in lib.images().iter() {
                if let Some(border) = image.border_width() {
                    let rect = image_rect(image.width().to_mm(), image.height().to_mm());
                    let img_xf = xf
                        * Affine::translate(convert::point(image.position()).to_vec2())
                        * Affine::rotate(convert::angle(image.rotation()));
                    let item = Item::new(
                        layer,
                        img_xf * rect.to_path(1e-4),
                        Style::stroke(shapes::pen(*border)),
                    )
                    .with_z(z::SYMBOL_IMAGES);
                    self.insert(Some(item), object);
                }
            }
        }

        // Pins.
        for pin in &pins {
            self.pin_positions
                .insert((symbol.uuid(), pin.uuid()), pin.position());
            self.add_pin(symbol, &transform, pin);
        }

        // Texts (with substituted attributes).
        let texts: Vec<Text> = symbol.texts().values().cloned().collect();
        for text in texts {
            let Some(layer) = self.layer(text.layer()) else {
                continue;
            };
            let value = attributes::substitute_symbol_text(
                self.project,
                self.schematic,
                symbol,
                text.text(),
            );
            let rendered = self.text.text(
                &value,
                text.position(),
                TextStyle {
                    height: text.height().to_mm(),
                    align: text.align(),
                    rotation: text.rotation(),
                    auto_rotate: true,
                    mirror_in_place: false,
                    parse_overlines: false,
                },
            );
            self.insert_text(layer, z::TEXTS, rendered, object);
        }
        Ok(())
    }

    fn add_pin(
        &mut self,
        symbol: &SchematicSymbol,
        transform: &librepcb_core::utils::transform::Transform,
        pin: &SymbolPinView<'_>,
    ) {
        let object = SchematicObject::SymbolPin(symbol.id(), pin.uuid());
        let lib_pin = pin.lib_pin();
        let rotation = pin.rotation();
        let start = pin.position();

        // Line.
        if let Some(layer) = SchematicScene::layer_id("schematic_pin_lines") {
            let end = start
                + Point::new(*lib_pin.length(), Length::ZERO).rotated(rotation, Point::ORIGIN);
            let item = shapes::line(layer, start, end, PIN_LINE_WIDTH).with_z(z::PINS);
            self.insert(Some(item), object);
        }

        // Name.
        if let Some(layer) = SchematicScene::layer_id("schematic_pin_names") {
            let mut align = lib_pin.name_alignment();
            if transform.mirrored {
                align = align.mirrored_v();
            }
            let pos = transform.map(
                &(lib_pin.position()
                    + lib_pin
                        .name_position()
                        .rotated(lib_pin.rotation(), Point::ORIGIN)),
            );
            let rendered = self.text.text(
                pin.name(),
                pos,
                TextStyle {
                    height: lib_pin.name_height().to_mm(),
                    align,
                    rotation: transform
                        .map_non_mirrorable(lib_pin.rotation() + lib_pin.name_rotation()),
                    auto_rotate: true,
                    mirror_in_place: false,
                    parse_overlines: true,
                },
            );
            self.insert_text(layer, z::PIN_TEXTS, rendered, object);
        }

        // Numbers.
        if let Some(layer) = SchematicScene::layer_id("schematic_pin_numbers") {
            let signal = pin.signal();
            let names = self.pad_names(signal.component.0, signal.signal).to_vec();
            let numbers = pin.numbers_text(&names);
            let pos = transform.map(
                &(lib_pin.position()
                    + pin
                        .numbers_position()
                        .rotated(lib_pin.rotation(), Point::ORIGIN)),
            );
            let rendered = self.text.text(
                &numbers,
                pos,
                TextStyle {
                    height: SymbolPin::numbers_height().to_mm(),
                    align: pin.numbers_alignment(),
                    rotation: transform.map_non_mirrorable(lib_pin.rotation()),
                    auto_rotate: true,
                    mirror_in_place: false,
                    parse_overlines: false,
                },
            );
            self.insert_text(layer, z::PIN_TEXTS, rendered, object);
        }

        // Junction dot if more than one net line is connected.
        let anchor = pin.anchor();
        if let Some(segment) = self.schematic.pin_net_segment(symbol.id(), pin.uuid())
            && let Some(s) = self.schematic.net_segments().get(&segment)
            && s.is_visible_junction(anchor)
            && let Some(layer) = SchematicScene::layer_id("schematic_wires")
        {
            let dot = junction(layer, start, z::NET_JUNCTIONS);
            self.insert(Some(dot), SchematicObject::NetJunction(segment, anchor));
        }
    }

    /// The sorted pad names of a component signal on the primary device
    /// (upstream `ComponentSignalInstance::getPadNames()`).
    fn pad_names(&mut self, component: Uuid, signal: Uuid) -> &[String] {
        if !self.pad_names.contains_key(&(component, signal)) {
            let project = self.project;
            let circuit = project.circuit();
            let mut per_signal: HashMap<Uuid, BTreeSet<String>> = HashMap::new();
            if let Some(cmp) =
                circuit.component_instance(librepcb_core::project::ComponentInstanceId(component))
                && let Some(device) = attributes::primary_device(project, cmp)
                && let Ok(pads) = device.pads(project.library(), circuit)
            {
                for pad in pads {
                    if let (Some(sig), Some(pkg_pad)) = (pad.component_signal(), pad.package_pad())
                    {
                        per_signal
                            .entry(sig.signal)
                            .or_default()
                            .insert(pkg_pad.name().to_string());
                    }
                }
            }
            // Cache all signals of the component at once.
            let signals: Vec<Uuid> = circuit
                .component_instance(librepcb_core::project::ComponentInstanceId(component))
                .map(|c| c.signals().keys().copied().collect())
                .unwrap_or_default();
            for s in signals.into_iter().chain([signal]) {
                let mut names: Vec<String> = per_signal
                    .get(&s)
                    .map(|n| n.iter().cloned().collect())
                    .unwrap_or_default();
                names.sort_by(|a, b| toolbox::compare_numeric(a, b));
                self.pad_names.insert((component, s), names);
            }
        }
        self.pad_names
            .get(&(component, signal))
            .map_or(&[], Vec::as_slice)
    }

    fn add_page_items(&mut self) {
        let project = self.project;
        let schematic = self.schematic;
        // Image borders.
        if let Some(layer) = SchematicScene::layer_id("schematic_image_borders") {
            for (uuid, image) in schematic.images() {
                if let Some(border) = image.border_width() {
                    let rect = image_rect(image.width().to_mm(), image.height().to_mm());
                    let xf = Affine::translate(convert::point(image.position()).to_vec2())
                        * Affine::rotate(convert::angle(image.rotation()));
                    let item = Item::new(
                        layer,
                        xf * rect.to_path(1e-4),
                        Style::stroke(shapes::pen(*border)),
                    )
                    .with_z(z::IMAGES);
                    self.insert(Some(item), SchematicObject::Image(*uuid));
                }
            }
        }
        // Polygons.
        for (uuid, polygon) in schematic.polygons() {
            let Some(layer) = self.layer(polygon.layer()) else {
                continue;
            };
            let fill = if polygon.is_filled() {
                Fill::Layer
            } else if polygon.is_grab_area() {
                self.grab_area_fill(polygon.layer())
            } else {
                Fill::None
            };
            let item = shapes::polygon(
                layer,
                polygon.path(),
                Affine::IDENTITY,
                *polygon.line_width(),
                fill,
            )
            .map(|i| i.with_z(z::POLYGONS));
            self.insert(item, SchematicObject::Polygon(*uuid));
        }
        // Texts.
        for (uuid, text) in schematic.texts() {
            let Some(layer) = self.layer(text.layer()) else {
                continue;
            };
            let value = attributes::substitute_schematic_text(project, schematic, text.text());
            let rendered = self.text.text(
                &value,
                text.position(),
                TextStyle {
                    height: text.height().to_mm(),
                    align: text.align(),
                    rotation: text.rotation(),
                    auto_rotate: true,
                    mirror_in_place: false,
                    parse_overlines: false,
                },
            );
            self.insert_text(layer, z::TEXTS, rendered, SchematicObject::Text(*uuid));
        }
    }

    fn anchor_position(&self, segment: NetSegmentId, anchor: NetLineAnchor) -> Option<Point> {
        match anchor {
            NetLineAnchor::Pin { symbol, pin } => self.pin_positions.get(&(symbol, pin)).copied(),
            _ => self
                .schematic
                .net_line_anchor_position(segment, anchor, self.project.view()),
        }
    }

    fn add_net_segments(&mut self) {
        let project = self.project;
        let wires = SchematicScene::layer_id("schematic_wires");
        let labels = SchematicScene::layer_id("schematic_net_labels");
        for (id, segment) in self.schematic.net_segments() {
            let id = *id;
            if let Some(layer) = wires {
                for (uuid, line) in segment.lines() {
                    match (
                        self.anchor_position(id, line.p1()),
                        self.anchor_position(id, line.p2()),
                    ) {
                        (Some(p1), Some(p2)) => {
                            let item =
                                shapes::line(layer, p1, p2, *line.width()).with_z(z::NET_LINES);
                            self.insert(Some(item), SchematicObject::NetLine(id, *uuid));
                        }
                        _ => self
                            .warnings
                            .push(format!("Net line {uuid}: unresolved anchor")),
                    }
                }
                for (uuid, junction_obj) in segment.junctions() {
                    let anchor = NetLineAnchor::Junction(*uuid);
                    if segment.is_visible_junction(anchor) {
                        let dot = junction(layer, junction_obj.position(), z::NET_JUNCTIONS);
                        self.insert(Some(dot), SchematicObject::NetJunction(id, anchor));
                    }
                }
            }
            if let Some(layer) = labels {
                let name = project
                    .circuit()
                    .net_signal(segment.net())
                    .map(|n| n.name().as_str().to_owned())
                    .unwrap_or_default();
                for (uuid, label) in segment.labels() {
                    let rendered = self.text.net_label(
                        &name,
                        label.position(),
                        label.rotation(),
                        label.mirrored(),
                    );
                    self.insert_text(
                        layer,
                        z::NET_LABELS,
                        rendered,
                        SchematicObject::NetLabel(id, *uuid),
                    );
                }
            }
        }
    }

    fn add_bus_segments(&mut self) {
        let project = self.project;
        let buses = SchematicScene::layer_id("schematic_buses");
        let labels = SchematicScene::layer_id("schematic_bus_labels");
        for (id, segment) in self.schematic.bus_segments() {
            let id = *id;
            if let Some(layer) = buses {
                for (uuid, line) in segment.lines() {
                    match (
                        segment.junction_position(line.p1()),
                        segment.junction_position(line.p2()),
                    ) {
                        (Some(p1), Some(p2)) => {
                            let item =
                                shapes::line(layer, p1, p2, *line.width()).with_z(z::BUS_LINES);
                            self.insert(Some(item), SchematicObject::BusLine(id, *uuid));
                        }
                        _ => self
                            .warnings
                            .push(format!("Bus line {uuid}: unresolved anchor")),
                    }
                }
                for (uuid, j) in segment.junctions() {
                    if segment.is_visible_junction(*uuid) {
                        let dot = junction(layer, j.position(), z::BUS_JUNCTIONS);
                        self.insert(Some(dot), SchematicObject::BusJunction(id, *uuid));
                    }
                }
            }
            if let Some(layer) = labels {
                let name = project
                    .circuit()
                    .bus(segment.bus())
                    .map(|b| b.name().as_str().to_owned())
                    .unwrap_or_default();
                for (uuid, label) in segment.labels() {
                    let rendered = self.text.net_label(
                        &name,
                        label.position(),
                        label.rotation(),
                        label.mirrored(),
                    );
                    self.insert_text(
                        layer,
                        z::BUS_LABELS,
                        rendered,
                        SchematicObject::BusLabel(id, *uuid),
                    );
                }
            }
        }
    }
}

/// A filled junction dot (upstream `drawNetJunction()`).
fn junction(layer: LayerId, position: Point, z: i32) -> Item {
    Item::new(
        layer,
        KCircle::new(convert::point(position), JUNCTION_RADIUS),
        Style::fill(),
    )
    .with_z(z)
}

/// The rectangle of an image in image coordinates (bottom left at the
/// origin, like upstream `drawImage()`).
fn image_rect(width: f64, height: f64) -> Rect {
    Rect::new(0.0, 0.0, width, height)
}
