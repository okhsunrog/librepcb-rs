//! Port of libs/librepcb/core/project/board/items/bi_plane.{h,cpp}.
//!
//! TODO(wave3b/board): placeholder. The final type stores the layer,
//! optional net signal, outline, clearances, connect style, priority,
//! thermal settings, lock and visibility flags; the calculated fragments
//! are derived data of the board.

use crate::project::id::PlaneId;
use crate::types::Uuid;

/// A copper plane of a board (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardPlane {
    uuid: Uuid,
}

impl BoardPlane {
    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> PlaneId {
        PlaneId(self.uuid)
    }
}
