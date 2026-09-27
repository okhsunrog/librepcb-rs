//! Port of libs/librepcb/core/project/schematic/schematic.{h,cpp}.
//!
//! Differences to upstream: no project back-pointer, no `isAddedToProject`
//! state, no own `TransactionalDirectory` (the project writes
//! `schematics/<directory_name>/` on save); the item `add*()`/`remove*()`
//! operations live in the project mutations and the Qt signals in the
//! change journal.
//!
//! TODO(wave3b/schematic): polygons, texts, images and the item contents;
//! `save()` currently writes the loaded file back verbatim.

use std::collections::BTreeMap;

use super::{SchematicBusSegment, SchematicNetSegment, SchematicSymbol};
use crate::fileio::{self, FileSystem, TransactionalDirectory};
use crate::geometry::property;
use crate::project::id::{BusSegmentId, ComponentSignalRef, NetSegmentId, SchematicId, SymbolId};
use crate::project::ref_index::Reference;
use crate::serialization::{List, Mode, SExpression};
use crate::types::{ElementName, Length, LengthUnit, PositiveLength, Uuid};

/// A schematic page of a project.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schematic {
    pub(crate) uuid: Uuid,
    /// Name of the directory below `schematics/`.
    pub(crate) directory_name: String,
    pub(crate) name: ElementName,
    pub(crate) grid_interval: PositiveLength,
    pub(crate) grid_unit: LengthUnit,
    pub(crate) symbols: BTreeMap<SymbolId, SchematicSymbol>,
    pub(crate) bus_segments: BTreeMap<BusSegmentId, SchematicBusSegment>,
    pub(crate) net_segments: BTreeMap<NetSegmentId, SchematicNetSegment>,
    /// The loaded `schematic.lp`, written back verbatim by [`save()`](Self::save)
    /// until the items are ported (TODO(wave3b/schematic): remove).
    #[serde(skip)]
    pub(crate) raw: Option<SExpression>,
}

impl Schematic {
    /// Creates an empty schematic (default grid: 2.54 mm).
    pub fn new(uuid: Uuid, name: ElementName, directory_name: impl Into<String>) -> Self {
        Self {
            uuid,
            directory_name: directory_name.into(),
            name,
            grid_interval: PositiveLength::new(Length::new(2_540_000))
                .expect("constant is positive"),
            grid_unit: LengthUnit::Millimeters,
            symbols: BTreeMap::new(),
            bus_segments: BTreeMap::new(),
            net_segments: BTreeMap::new(),
            raw: None,
        }
    }

    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> SchematicId {
        SchematicId(self.uuid)
    }

    /// Returns the name of the directory below `schematics/`.
    pub fn directory_name(&self) -> &str {
        &self.directory_name
    }

    property!(
        /// Returns the name.
        ref name: ElementName, set_name
    );
    property!(
        /// Returns the grid interval.
        copy grid_interval: PositiveLength, set_grid_interval
    );
    property!(
        /// Returns the grid unit.
        copy grid_unit: LengthUnit, set_grid_unit
    );

    /// Returns the symbols.
    pub fn symbols(&self) -> &BTreeMap<SymbolId, SchematicSymbol> {
        &self.symbols
    }

    /// Returns the bus segments.
    pub fn bus_segments(&self) -> &BTreeMap<BusSegmentId, SchematicBusSegment> {
        &self.bus_segments
    }

    /// Returns the net segments.
    pub fn net_segments(&self) -> &BTreeMap<NetSegmentId, SchematicNetSegment> {
        &self.net_segments
    }

    /// Whether the schematic contains no items (upstream `isEmpty()`).
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.bus_segments.is_empty() && self.net_segments.is_empty()
    }

    /// Returns the editable properties.
    pub fn properties(&self) -> SchematicProperties {
        SchematicProperties {
            id: self.id(),
            name: self.name.clone(),
            grid_interval: self.grid_interval,
            grid_unit: self.grid_unit,
        }
    }

    /// Replaces the editable properties, returns the previous ones.
    pub fn set_properties(&mut self, p: SchematicProperties) -> SchematicProperties {
        let old = self.properties();
        self.name = p.name;
        self.grid_interval = p.grid_interval;
        self.grid_unit = p.grid_unit;
        old
    }

    /// All references of the items into the circuit, for the project's
    /// reverse index.
    pub(crate) fn references(&self, _id: SchematicId) -> Vec<Reference> {
        // TODO(wave3b/schematic): net segments -> nets, bus segments ->
        // buses, symbols -> component gates.
        Vec::new()
    }

    /// Whether net lines are connected to a pin of the given component
    /// signal (upstream `ComponentSignalInstance::arePinsOrPadsUsed()`).
    pub(crate) fn is_component_signal_wired(&self, _signal: ComponentSignalRef) -> bool {
        // TODO(wave3b/schematic)
        false
    }

    /// Writes the header of `schematic.lp` (upstream `Schematic::save()`
    /// up to the items).
    pub(crate) fn serialize_header(&self, root: &mut List) {
        root.append_value(&self.uuid);
        root.ensure_line_break();
        root.append_child("name", &self.name);
        root.ensure_line_break();
        let grid = root.append_list("grid");
        grid.append_child("interval", &self.grid_interval);
        grid.append_child("unit", &self.grid_unit);
        root.ensure_line_break();
    }

    /// Writes `schematic.lp` and `settings.user.lp` into `dir`.
    pub(crate) fn save(&self, dir: &mut TransactionalDirectory) -> fileio::Result<()> {
        let root = match &self.raw {
            Some(raw) => raw.clone(),
            None => {
                let mut root = List::new("librepcb_schematic");
                self.serialize_header(&mut root);
                // TODO(wave3b/schematic): items.
                root.ensure_line_break();
                SExpression::from(root)
            }
        };
        dir.write("schematic.lp", &root.to_byte_array(Mode::LibrePcb)?)?;
        let mut user = List::new("librepcb_schematic_user_settings");
        user.ensure_line_break();
        dir.write(
            "settings.user.lp",
            &SExpression::from(user).to_byte_array(Mode::LibrePcb)?,
        )?;
        Ok(())
    }
}

/// The editable properties of a [`Schematic`] (upstream `CmdSchematicEdit`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchematicProperties {
    /// The schematic.
    pub id: SchematicId,
    /// The name.
    pub name: ElementName,
    /// The grid interval.
    pub grid_interval: PositiveLength,
    /// The grid unit.
    pub grid_unit: LengthUnit,
}
