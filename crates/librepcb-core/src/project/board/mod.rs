//! Port of libs/librepcb/core/project/board (boards and their items).
//!
//! A [`Board`] stores its settings ([`BoardSettings`]: layers, design
//! rules, DRC and fabrication output settings, ...), the DRC approvals and
//! its items as plain data: [`BoardDevice`]s keyed by component instance,
//! [`BoardNetSegment`]s (standalone pads, vias, junctions, traces),
//! [`BoardPlane`]s, zones, polygons, stroke texts and holes (the upstream
//! `Board*Data` types). Footprint pads are computed
//! [`FootprintPadView`]s. Air wires and plane fragments are derived data
//! ([`BoardDerived`]), recomputed on demand from dirty sets maintained by
//! the mutations ([`BoardAirWiresBuilder`]).
//!
//! The plane fragments are calculated by [`PlaneJob`]s
//! ([`Project::plane_job()`](crate::project::Project::plane_job),
//! [`Project::rebuild_planes()`](crate::project::Project::rebuild_planes)).
//! The fabrication and assembly exports drive the generators of
//! [`export`](crate::export): [`BoardGerberExport`] (Gerber/Excellon),
//! [`BoardPickPlaceGenerator`] and [`BoardD356NetlistExport`].
//!
//! Not ported yet: the painters, the Specctra and interactive HTML BOM
//! exports and the DRC.

mod air_wire;
mod air_wires_builder;
#[allow(clippy::module_inception)] // Upstream file name.
mod board;
mod board_context;
mod change;
mod d356_netlist_export;
mod design_rules;
mod device;
mod export_error;
mod fabrication_output_settings;
mod gerber_export;
mod hole_data;
pub(crate) mod keyed_map;
mod net_segment;
mod net_segment_splitter;
mod pad_data;
mod pick_place_generator;
mod plane;
mod plane_fragments_builder;
mod polygon_data;
mod stroke_text_data;
mod zone_data;

pub use air_wire::AirWire;
pub use air_wires_builder::BoardAirWiresBuilder;
pub use board::{
    Board, BoardDerived, BoardItem, BoardProperties, BoardSettings, DEFAULT_STROKE_FONT_NAME,
    PreferredFootprintTags,
};
pub use change::{BoardChange, BoardItemKind, BoardSegmentItems};
pub use d356_netlist_export::{BoardD356NetlistExport, export_d356_netlist};
pub use design_rules::BoardDesignRules;
pub use device::{BoardDevice, FootprintPadView};
pub use export_error::{BoardExportError, BoardExportResult};
pub use fabrication_output_settings::BoardFabricationOutputSettings;
pub use gerber_export::{
    BeforeWriteCallback, BoardGerberExport, ExportInfo, export_component_layer,
    export_fabrication_data,
};
pub use hole_data::BoardHoleData;
pub use net_segment::{BoardNetSegment, BoardSegmentElements};
pub use net_segment_splitter::{BoardNetSegmentParts, BoardNetSegmentSplitter};
pub use pad_data::BoardPadData;
pub use pick_place_generator::{BoardPickPlaceGenerator, export_pick_place_csv};
pub use plane::{BoardPlane, PlaneConnectStyle};
pub use plane_fragments_builder::{PlaneFragments, PlaneJob};
pub use polygon_data::BoardPolygonData;
pub use stroke_text_data::BoardStrokeTextData;
pub use zone_data::BoardZoneData;

pub(crate) use board::{FootprintPadKey, PadUse};
pub(crate) use pad_data::PadOnBoard;
