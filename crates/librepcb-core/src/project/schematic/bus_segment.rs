//! Port of libs/librepcb/core/project/schematic/items/si_bussegment.{h,cpp}
//! (with `si_busjunction`, `si_busline` and `si_buslabel`).
//!
//! TODO(wave3b/schematic): placeholder. The final type stores the bus,
//! junctions, lines and labels.

use crate::project::id::BusSegmentId;
use crate::types::Uuid;

/// A bus segment of a schematic (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchematicBusSegment {
    uuid: Uuid,
}

impl SchematicBusSegment {
    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> BusSegmentId {
        BusSegmentId(self.uuid)
    }
}
