//! Read-only view of a board with the derived item properties which upstream
//! caches in its `BI_*` objects (`BI_Pad` geometries, `BI_Via` actual
//! size/drill and stop mask diameters, `BI_StrokeText` paths), as needed by
//! the plane fragments builder and the exports.
//!
//! Also contains the attribute lookups of boards and devices (the parts of
//! upstream `ProjectAttributeLookup` which the exports use: board, device,
//! component, part and project attributes), to substitute stroke texts and
//! output file paths.
//!
//! Not an upstream class: upstream computes these values in the item
//! objects (see `bi_pad.cpp`, `bi_via.cpp`, `bi_stroketext.cpp`,
//! `bi_device.cpp`) and `projectattributelookup.cpp`; the values and their
//! order are the same.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, Utc};

use super::{Board, BoardDevice, BoardStrokeTextData};
use crate::attribute::{self, AttributeList};
use crate::font::StrokeTextPathBuilder;
use crate::geometry::{ComponentSide, Pad, PadGeometry, Path, TraceAnchor, Via};
use crate::library::LibraryBaseElement;
use crate::library::dev::{Device, Part};
use crate::library::pkg::{Footprint, Package};
use crate::project::Project;
use crate::project::board::export_error::BoardExportResult;
use crate::project::board::pad_data::PadOnBoard;
use crate::project::circuit::ComponentInstance;
use crate::project::error::{EntityKind, Error};
use crate::project::id::{AssemblyVariantId, ComponentInstanceId, NetSignalId};
use crate::types::{Angle, Layer, Length, Point, PositiveLength, Uuid};
use crate::utils::transform::Transform;

/// A device of a board with its resolved library elements and component.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextDevice<'a> {
    pub device: &'a BoardDevice,
    pub component: &'a ComponentInstance,
    pub lib_device: &'a Device,
    pub package: &'a Package,
    pub footprint: &'a Footprint,
}

impl<'a> ContextDevice<'a> {
    /// Returns the designator (component instance name).
    pub fn designator(&self) -> &'a str {
        self.component.name().as_str()
    }

    /// Returns the transformation of the device.
    pub fn transform(&self) -> Transform {
        self.device.transform()
    }

    /// Returns the parts of the device in an assembly variant (all variants
    /// if `None`), upstream `BI_Device::getParts()`.
    pub fn parts(&self, assembly_variant: Option<AssemblyVariantId>) -> Vec<Part> {
        let lib_device = self.device.lib_device();
        let mut parts = Vec::new();
        for option in self.component.assembly_options() {
            if (option.device() == lib_device)
                && assembly_variant.is_none_or(|av| option.assembly_variants().contains(&av))
            {
                parts.extend(option.parts().iter().cloned());
                if option.parts().is_empty() {
                    parts.push(Part::new(
                        Default::default(),
                        Default::default(),
                        option.attributes().clone(),
                    ));
                }
            }
        }
        parts
    }

    /// Whether the device is assembled in an assembly variant (upstream
    /// `BI_Device::isInAssemblyVariant()`).
    pub fn is_in_assembly_variant(&self, assembly_variant: AssemblyVariantId) -> bool {
        let lib_device = self.device.lib_device();
        self.component.assembly_options().iter().any(|option| {
            (option.device() == lib_device)
                && option.assembly_variants().contains(&assembly_variant)
        })
    }

    /// Returns the stop mask offsets of the footprint holes (upstream
    /// `BI_Device::getHoleStopMasks()`).
    pub fn hole_stop_mask_offsets(
        &self,
        rules: &super::BoardDesignRules,
    ) -> BTreeMap<Uuid, Option<Length>> {
        BoardDevice::hole_stop_mask_offsets(self.footprint, rules)
    }
}

/// A pad on a board (upstream `BI_Pad`): a footprint pad of a device or a
/// standalone pad of a net segment.
#[derive(Debug, Clone)]
pub(crate) struct ContextPad<'a> {
    /// The pad properties (in pad coordinates).
    pub properties: &'a Pad,
    /// Position on the board.
    pub position: Point,
    /// Rotation on the board.
    pub rotation: Angle,
    /// Whether the pad is mirrored (device on the bottom side).
    pub mirrored: bool,
    /// The net of the pad.
    pub net: Option<NetSignalId>,
    /// The device, for footprint pads.
    pub device: Option<ContextDevice<'a>>,
    /// Name of the package pad (footprint pads connected to a package pad).
    pub package_pad_name: Option<&'a str>,
    /// Name of the component signal (footprint pads connected to a signal).
    pub signal_name: Option<&'a str>,
    /// Geometries on the copper, stop mask and solder paste layers.
    pub geometries: BTreeMap<Layer, Vec<PadGeometry>>,
}

impl ContextPad<'_> {
    fn on_board(&self) -> PadOnBoard<'_> {
        PadOnBoard {
            pad: self.properties,
            mirrored: self.mirrored,
        }
    }

    /// Returns the transformation of the pad (upstream `Transform(BI_Pad)`).
    pub fn transform(&self) -> Transform {
        Transform::new(self.position, self.rotation, self.mirrored)
    }

    /// Upstream `BI_Pad::isOnLayer()`.
    pub fn is_on_layer(&self, layer: Layer) -> bool {
        self.on_board().is_on_layer(layer)
    }

    /// Upstream `BI_Pad::getComponentSide()`.
    pub fn component_side(&self) -> ComponentSide {
        self.on_board().component_side()
    }

    /// Returns the geometries on a layer.
    pub fn geometries_on(&self, layer: Layer) -> &[PadGeometry] {
        self.geometries.get(&layer).map_or(&[], Vec::as_slice)
    }
}

/// A via with its derived properties (upstream `BI_Via`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextVia<'a> {
    pub via: &'a Via,
    /// Upstream `getActualDrillDiameter()`.
    pub drill: PositiveLength,
    /// Upstream `getActualSize()`.
    pub size: PositiveLength,
    /// Upstream `getStopMaskDiameterTop()`.
    pub stop_mask_top: Option<PositiveLength>,
    /// Upstream `getStopMaskDiameterBottom()`.
    pub stop_mask_bot: Option<PositiveLength>,
}

/// A read-only view of a board of a project, see the module documentation.
#[derive(Debug, Clone)]
pub(crate) struct BoardContext<'a> {
    pub project: &'a Project,
    pub board: &'a Board,
    /// Devices in UUID order.
    pub devices: Vec<ContextDevice<'a>>,
    /// Positions of the footprint pads (for trace anchors).
    footprint_pad_positions: BTreeMap<(ComponentInstanceId, Uuid), Point>,
}

impl<'a> BoardContext<'a> {
    /// Resolves the devices of `board`; fails if a library element or
    /// component instance does not exist.
    pub fn new(project: &'a Project, board: &'a Board) -> BoardExportResult<Self> {
        let library = project.library();
        let circuit = project.circuit();
        let mut devices = Vec::with_capacity(board.devices().len());
        let mut footprint_pad_positions = BTreeMap::new();
        for device in board.devices().values() {
            let component =
                circuit
                    .component_instance(device.component())
                    .ok_or(Error::NotFound {
                        kind: EntityKind::Component,
                        uuid: device.component().0,
                    })?;
            let lib_device = library
                .device(&device.lib_device())
                .ok_or(Error::MissingLibraryDevice(device.lib_device()))?;
            let package = library
                .package(&lib_device.package_uuid())
                .ok_or(Error::MissingLibraryPackage(lib_device.package_uuid()))?;
            let footprint = package
                .footprints()
                .required_by_uuid(&device.lib_footprint())
                .map_err(Error::from)?;
            let transform = device.transform();
            for pad in footprint.pads() {
                footprint_pad_positions.insert(
                    (device.component(), pad.uuid()),
                    transform.map(&pad.pad().position()),
                );
            }
            devices.push(ContextDevice {
                device,
                component,
                lib_device,
                package,
                footprint,
            });
        }
        Ok(Self {
            project,
            board,
            devices,
            footprint_pad_positions,
        })
    }

    /// Returns the name of a net.
    pub fn net_name(&self, net: Option<NetSignalId>) -> Option<&'a str> {
        net.and_then(|n| self.project.circuit().net_signal(n))
            .map(|n| n.name().as_str())
    }

    /// Returns the footprint pads of a device, sorted by UUID (upstream
    /// `BI_Device::getPads()`).
    pub fn device_pads(
        &self,
        device: &ContextDevice<'a>,
    ) -> BoardExportResult<Vec<ContextPad<'a>>> {
        let copper_layers = self.board.copper_layers();
        let rules = self.board.design_rules();
        let mut views = device
            .device
            .pads(self.project.library(), self.project.circuit())?;
        views.sort_by_key(|v| v.uuid());
        Ok(views
            .into_iter()
            .map(|view| {
                let connected = self.board.anchor_trace_layers(view.trace_anchor());
                ContextPad {
                    properties: view.properties(),
                    position: view.position(),
                    rotation: view.rotation(),
                    mirrored: view.mirrored(),
                    net: view.net(),
                    device: Some(*device),
                    package_pad_name: view.package_pad().map(|p| p.name().as_str()),
                    signal_name: view.component_signal_name(),
                    geometries: view.geometries(&copper_layers, rules, &connected),
                }
            })
            .collect())
    }

    /// Returns the standalone pads of a net segment, sorted by UUID.
    pub fn segment_pads(&self, segment: &'a super::BoardNetSegment) -> Vec<ContextPad<'a>> {
        let copper_layers = self.board.copper_layers();
        let rules = self.board.design_rules();
        segment
            .pads()
            .values()
            .map(|pad| {
                let connected = self.board.anchor_trace_layers(TraceAnchor::Pad(pad.uuid()));
                let on_board = PadOnBoard {
                    pad: pad.pad(),
                    mirrored: false,
                };
                ContextPad {
                    properties: pad.pad(),
                    position: pad.pad().position(),
                    rotation: pad.pad().rotation(),
                    mirrored: false,
                    net: segment.net(),
                    device: None,
                    package_pad_name: None,
                    signal_name: None,
                    geometries: on_board.geometries(&copper_layers, rules, &connected),
                }
            })
            .collect()
    }

    /// Returns the vias of a net segment with their derived properties,
    /// sorted by UUID.
    pub fn segment_vias(&self, segment: &'a super::BoardNetSegment) -> Vec<ContextVia<'a>> {
        let rules = self.board.design_rules();
        let circuit = self.project.circuit();
        let net_class_drill = segment
            .net()
            .and_then(|n| circuit.net_signal(n))
            .and_then(|n| circuit.net_class(n.net_class()))
            .and_then(|c| c.default_via_drill());
        segment
            .vias()
            .values()
            .map(|via| {
                // Upstream `BI_Via::updateActualDrillAndSize()`.
                let drill = via
                    .drill_diameter()
                    .or(net_class_drill)
                    .unwrap_or(rules.default_via_drill_diameter());
                let size = match via.size() {
                    // Avoid invalid via if drill is auto.
                    Some(size) => size.max(drill),
                    None => Via::calc_size_from_rules(drill, &rules.via_annular_ring()),
                };
                // Upstream `BI_Via::updateStopMaskDiameters()`.
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
                let dia = PositiveLength::new(dia).ok();
                ContextVia {
                    via,
                    drill,
                    size,
                    stop_mask_top: dia.filter(|_| via.start_layer().is_top()),
                    stop_mask_bot: dia.filter(|_| via.end_layer().is_bottom()),
                }
            })
            .collect()
    }

    /// Returns the drill layer span of a blind or buried via, `None` if the
    /// via is invalid with the current layer count (upstream
    /// `BI_Via::getDrillLayerSpan()`).
    pub fn via_drill_layer_span(&self, via: &Via) -> Option<(Layer, Layer)> {
        let inner = self.board.settings().inner_layer_count as usize;
        // If start layer is not enabled, the via is invalid.
        let start_number = via.start_layer().copper_number();
        if start_number > inner {
            return None;
        }
        // If the via ends at the bottom layer, the via is valid.
        if via.end_layer().is_bottom() {
            return Some((via.start_layer(), via.end_layer()));
        }
        // Via ends on an inner layer --> check layer span.
        let end_number = via.end_layer().copper_number().min(inner);
        match Layer::inner_copper(end_number) {
            Some(end) if start_number < end_number => Some((via.start_layer(), end)),
            _ => None,
        }
    }

    /// Returns the position of a trace anchor of a net segment.
    pub fn anchor_position(
        &self,
        segment: &super::BoardNetSegment,
        anchor: TraceAnchor,
    ) -> Option<Point> {
        match anchor {
            TraceAnchor::FootprintPad { device, pad } => self
                .footprint_pad_positions
                .get(&(ComponentInstanceId(device), pad))
                .copied(),
            TraceAnchor::Junction(uuid) => segment.junctions().get(&uuid).map(|j| j.position()),
            TraceAnchor::Via(uuid) => segment.vias().get(&uuid).map(|v| v.position()),
            TraceAnchor::Pad(uuid) => segment.pads().get(&uuid).map(|p| p.pad().position()),
        }
    }

    /// Returns the paths of a stroke text (relative to its position, not
    /// transformed, upstream `BI_StrokeText::getPaths()`): the text with
    /// substituted attributes of the device (if any) or the board.
    pub fn stroke_text_paths(
        &self,
        text: &BoardStrokeTextData,
        device: Option<&ContextDevice<'a>>,
    ) -> BoardExportResult<Vec<Path>> {
        let font = self
            .project
            .stroke_fonts()
            .font(&self.board.settings().default_font)?;
        let substituted = match device {
            Some(dev) => {
                let part = dev.parts(None).into_iter().next();
                attribute::substitute(
                    text.text(),
                    |key| self.device_attribute(dev, part.as_ref(), key),
                    None,
                )
            }
            None => attribute::substitute(text.text(), |key| self.board_attribute(key), None),
        };
        Ok(StrokeTextPathBuilder::build(
            font,
            text.letter_spacing(),
            text.line_spacing(),
            text.height(),
            text.stroke_width(),
            text.align(),
            text.rotation(),
            text.auto_rotate(),
            &substituted,
        ))
    }

    /// Returns the transformation of a stroke text.
    pub fn stroke_text_transform(text: &BoardStrokeTextData) -> Transform {
        Transform::new(text.position(), text.rotation(), text.mirrored())
    }

    // ------------------------------------------------------------------
    // Attribute lookup (upstream ProjectAttributeLookup)
    // ------------------------------------------------------------------

    /// Board lookup without assembly variant (upstream
    /// `ProjectAttributeLookup(board, nullptr)`).
    pub fn board_attribute(&self, key: &str) -> Option<String> {
        self.query_board(key).or_else(|| self.query_project(key))
    }

    /// Device lookup (upstream `ProjectAttributeLookup(device, part)`).
    pub fn device_attribute(
        &self,
        device: &ContextDevice<'a>,
        part: Option<&Part>,
        key: &str,
    ) -> Option<String> {
        part.and_then(|p| query_part(p, key))
            .or_else(|| self.query_device(device, key))
            .or_else(|| self.query_component(device, key))
            .or_else(|| self.query_board(key))
            .or_else(|| self.query_project(key))
    }

    fn locale_order(&self) -> &[String] {
        &self.project.settings().locale_order
    }

    fn query_project(&self, key: &str) -> Option<String> {
        let project = self.project;
        let metadata = project.metadata();
        if let Some(value) = find_attribute(&metadata.attributes, key) {
            return Some(value);
        }
        let local = |dt: DateTime<Utc>| dt.with_timezone(&Local);
        let file_path = project.file_path();
        Some(match key {
            "PROJECT" => metadata.name.to_string(),
            "PROJECT_DIRPATH" => project.path().map(|p| p.to_native()).unwrap_or_default(),
            "PROJECT_BASENAME" => file_path
                .as_ref()
                .map(|p| p.basename().to_owned())
                .unwrap_or_default(),
            "PROJECT_FILENAME" => project.file_name().to_owned(),
            "PROJECT_FILEPATH" => file_path.map(|p| p.to_native()).unwrap_or_default(),
            "CREATED_DATE" => local(metadata.created).format("%Y-%m-%d").to_string(),
            "CREATED_TIME" => local(metadata.created).format("%H:%M:%S").to_string(),
            "DATE" => local(project.date_time()).format("%Y-%m-%d").to_string(),
            "TIME" => local(project.date_time()).format("%H:%M:%S").to_string(),
            "AUTHOR" => metadata.author.clone(),
            "VERSION" => metadata.version.to_string(),
            "PAGES" => project.schematics().len().to_string(),
            // Do not translate this, must be the same for every user!
            "PAGE_X_OF_Y" => "Page {{PAGE}} of {{PAGES}}".to_owned(),
            _ => return None,
        })
    }

    fn query_board(&self, key: &str) -> Option<String> {
        Some(match key {
            "BOARD" => self.board.properties().name.to_string(),
            "BOARD_DIRNAME" => self.board.directory_name().to_owned(),
            "BOARD_INDEX" => self
                .project
                .board_index(self.board.id())
                .unwrap_or_default()
                .to_string(),
            _ => return None,
        })
    }

    fn query_component(&self, device: &ContextDevice<'a>, key: &str) -> Option<String> {
        let cmp = device.component;
        if let Some(value) = find_attribute(cmp.attributes(), key) {
            return Some(value);
        }
        Some(match key {
            "NAME" => cmp.name().to_string(),
            "VALUE" => cmp.value().clone(),
            "COMPONENT" => self
                .project
                .library()
                .component(&cmp.lib_component())
                .map(|c| c.metadata().names().value(self.locale_order()).to_string())
                .unwrap_or_default(),
            _ => return None,
        })
    }

    fn query_device(&self, device: &ContextDevice<'a>, key: &str) -> Option<String> {
        if let Some(value) = find_attribute(device.device.attributes(), key) {
            return Some(value);
        }
        let locale = self.locale_order();
        Some(match key {
            "DEVICE" => device
                .lib_device
                .metadata()
                .names()
                .value(locale)
                .to_string(),
            "PACKAGE" => device.package.metadata().names().value(locale).to_string(),
            "FOOTPRINT" => device.footprint.names().value(locale).to_string(),
            _ => return None,
        })
    }
}

fn find_attribute(attributes: &AttributeList, key: &str) -> Option<String> {
    attributes.by_name(key, true).map(|a| a.value_tr(true))
}

fn query_part(part: &Part, key: &str) -> Option<String> {
    if let Some(value) = find_attribute(part.attributes(), key) {
        return Some(value);
    }
    Some(match key {
        "MPN" => part.mpn().to_string(),
        "MANUFACTURER" => part.manufacturer().to_string(),
        _ => return None,
    })
}
