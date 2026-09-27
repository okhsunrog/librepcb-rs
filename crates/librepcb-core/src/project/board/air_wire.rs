//! Port of libs/librepcb/core/project/board/items/bi_airwire.{h,cpp}.
//!
//! Air wires are derived data (see
//! [`BoardDerived`](super::BoardDerived)), computed per net by the board
//! air wires builder and never saved.
//!
//! TODO(wave3b/board): placeholder. The final type stores the net signal
//! and the two anchors (`geometry::TraceAnchor`) with their positions.

use crate::project::id::NetSignalId;

/// An air wire (unrouted connection) of a board (placeholder, see the
/// module doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirWire {
    net: NetSignalId,
}

impl AirWire {
    /// Returns the net signal.
    pub fn net(&self) -> NetSignalId {
        self.net
    }
}
