//! Port of libs/librepcb/core/project/board/items/bi_netsegment.{h,cpp}
//! (with `bi_netpoint`, `bi_netline`, `bi_via` and the standalone
//! `bi_pad`, which become the geometry types `Junction`, `Trace`, `Via` and
//! `BoardPadData` stored in the segment).
//!
//! TODO(wave3b/board): placeholder. The final type stores the optional net
//! signal, pads, vias, junctions and traces (with `geometry::TraceAnchor`
//! anchors), and answers connectivity queries.

use crate::project::id::NetSegmentId;
use crate::types::Uuid;

/// A net segment of a board (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardNetSegment {
    uuid: Uuid,
}

impl BoardNetSegment {
    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> NetSegmentId {
        NetSegmentId(self.uuid)
    }
}
