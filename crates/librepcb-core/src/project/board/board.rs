//! Port of libs/librepcb/core/project/board/board.{h,cpp}.
//!
//! Differences to upstream: no project back-pointer, no `isAddedToProject`
//! state, no own `TransactionalDirectory` (the project writes
//! `boards/<directory_name>/` on save); the item `add*()`/`remove*()`
//! operations live in the project mutations, the Qt signals in the change
//! journal, and the derived data (air wires, plane fragments) with its
//! dirty sets in [`BoardDerived`], which is neither saved nor compared.
//!
//! TODO(wave3b/board): design rules, DRC settings/approvals, fabrication
//! output settings, layers, grid, preferred footprint tags, zones,
//! polygons, stroke texts, holes and the item contents; `save()` currently
//! writes the loaded files back verbatim.

use std::collections::BTreeMap;

use super::{BoardDevice, BoardNetSegment, BoardPlane};
use crate::fileio::{self, FileSystem, TransactionalDirectory};
use crate::geometry::property;
use crate::project::id::{BoardId, ComponentInstanceId, ComponentSignalRef, NetSegmentId, PlaneId};
use crate::project::ref_index::Reference;
use crate::serialization::{List, Mode, SExpression};
use crate::types::{ElementName, Uuid};

/// A board of a project.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Board {
    pub(crate) uuid: Uuid,
    /// Name of the directory below `boards/`.
    pub(crate) directory_name: String,
    pub(crate) name: ElementName,
    /// Keyed by component instance (upstream `mDeviceInstances`).
    pub(crate) devices: BTreeMap<ComponentInstanceId, BoardDevice>,
    pub(crate) net_segments: BTreeMap<NetSegmentId, BoardNetSegment>,
    pub(crate) planes: BTreeMap<PlaneId, BoardPlane>,
    /// The loaded `board.lp`, written back verbatim by [`save()`](Self::save)
    /// until the items are ported (TODO(wave3b/board): remove).
    #[serde(skip)]
    pub(crate) raw: Option<SExpression>,
    /// The loaded `settings.user.lp` (TODO(wave3b/board): layer and plane
    /// visibility).
    #[serde(skip)]
    pub(crate) raw_user_settings: Option<SExpression>,
    /// Derived data, not persistent.
    #[serde(skip)]
    pub(crate) derived: BoardDerived,
}

impl Board {
    /// Creates an empty board.
    pub fn new(uuid: Uuid, name: ElementName, directory_name: impl Into<String>) -> Self {
        Self {
            uuid,
            directory_name: directory_name.into(),
            name,
            devices: BTreeMap::new(),
            net_segments: BTreeMap::new(),
            planes: BTreeMap::new(),
            raw: None,
            raw_user_settings: None,
            derived: BoardDerived::default(),
        }
    }

    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> BoardId {
        BoardId(self.uuid)
    }

    /// Returns the name of the directory below `boards/`.
    pub fn directory_name(&self) -> &str {
        &self.directory_name
    }

    property!(
        /// Returns the name.
        ref name: ElementName, set_name
    );

    /// Returns the devices, keyed by component instance.
    pub fn devices(&self) -> &BTreeMap<ComponentInstanceId, BoardDevice> {
        &self.devices
    }

    /// Returns the net segments.
    pub fn net_segments(&self) -> &BTreeMap<NetSegmentId, BoardNetSegment> {
        &self.net_segments
    }

    /// Returns the planes.
    pub fn planes(&self) -> &BTreeMap<PlaneId, BoardPlane> {
        &self.planes
    }

    /// Returns the derived data (air wires, plane fragments).
    pub fn derived(&self) -> &BoardDerived {
        &self.derived
    }

    /// Whether the board contains no items (upstream `isEmpty()`).
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty() && self.net_segments.is_empty() && self.planes.is_empty()
    }

    /// Returns the editable properties.
    pub fn properties(&self) -> BoardProperties {
        BoardProperties {
            id: self.id(),
            name: self.name.clone(),
        }
    }

    /// Replaces the editable properties, returns the previous ones.
    pub fn set_properties(&mut self, p: BoardProperties) -> BoardProperties {
        let old = self.properties();
        self.name = p.name;
        old
    }

    /// All references of the items into the circuit, for the project's
    /// reverse index.
    pub(crate) fn references(&self, _id: BoardId) -> Vec<Reference> {
        // TODO(wave3b/board): net segments and planes -> nets, devices ->
        // component instances.
        Vec::new()
    }

    /// Whether traces are connected to a pad of the given component signal
    /// (upstream `ComponentSignalInstance::arePinsOrPadsUsed()`).
    pub(crate) fn is_component_signal_wired(&self, _signal: ComponentSignalRef) -> bool {
        // TODO(wave3b/board)
        false
    }

    /// Writes the header of `board.lp` (upstream `Board::save()` up to the
    /// settings).
    pub(crate) fn serialize_header(&self, root: &mut List) {
        root.append_value(&self.uuid);
        root.ensure_line_break();
        root.append_child("name", &self.name);
        root.ensure_line_break();
    }

    /// Writes `board.lp` and `settings.user.lp` into `dir`.
    pub(crate) fn save(&self, dir: &mut TransactionalDirectory) -> fileio::Result<()> {
        let root = match &self.raw {
            Some(raw) => raw.clone(),
            None => {
                let mut root = List::new("librepcb_board");
                self.serialize_header(&mut root);
                // TODO(wave3b/board): settings and items.
                root.ensure_line_break();
                SExpression::from(root)
            }
        };
        dir.write("board.lp", &root.to_byte_array(Mode::LibrePcb)?)?;
        let user = match &self.raw_user_settings {
            Some(raw) => raw.clone(),
            None => {
                let mut user = List::new("librepcb_board_user_settings");
                user.ensure_line_break();
                SExpression::from(user)
            }
        };
        dir.write("settings.user.lp", &user.to_byte_array(Mode::LibrePcb)?)?;
        Ok(())
    }
}

/// Derived (computed, not persistent) data of a board with the dirty sets
/// that schedule its recomputation (upstream `mAirWires`,
/// `mScheduledNetSignalsForAirWireRebuild`, `BI_Plane::mFragments`,
/// `mScheduledLayersForPlanesRebuild`).
///
/// Never participates in equality: two boards with the same persistent
/// content are equal regardless of their derived data.
///
/// TODO(wave3b/board): air wires per net, plane fragments per plane and
/// the dirty sets.
#[derive(Debug, Clone, Default)]
pub struct BoardDerived {}

impl PartialEq for BoardDerived {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// The editable properties of a [`Board`] (upstream `CmdBoardEdit`).
///
/// TODO(wave3b/board): grid, layers, thickness, colors, design rules, ...
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardProperties {
    /// The board.
    pub id: BoardId,
    /// The name.
    pub name: ElementName,
}
