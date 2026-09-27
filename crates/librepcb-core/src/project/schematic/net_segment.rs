//! Port of libs/librepcb/core/project/schematic/items/si_netsegment.{h,cpp}
//! (with `si_netpoint`, `si_netline` and `si_netlabel`, which become the
//! geometry types `Junction`, `NetLine` and `NetLabel` stored in the
//! segment).
//!
//! TODO(wave3b/schematic): placeholder. The final type stores the net
//! signal, junctions, lines (with `geometry::NetLineAnchor` anchors) and
//! labels, and answers connectivity queries (`lines_at()`, cohesion).

use crate::project::id::NetSegmentId;
use crate::types::Uuid;

/// A net segment of a schematic (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchematicNetSegment {
    uuid: Uuid,
}

impl SchematicNetSegment {
    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> NetSegmentId {
        NetSegmentId(self.uuid)
    }
}
