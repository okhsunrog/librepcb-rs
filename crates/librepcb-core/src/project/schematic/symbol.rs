//! Port of libs/librepcb/core/project/schematic/items/si_symbol.{h,cpp}
//! (and `si_symbolpin.{h,cpp}`, whose pins become computed views).
//!
//! TODO(wave3b/schematic): placeholder. The final type stores the
//! component instance, gate (symbol variant item), position, rotation,
//! mirror flag and texts; pins are views computed from the library symbol
//! and the component's pin-signal map.

use crate::project::id::SymbolId;
use crate::types::Uuid;

/// A symbol placed in a schematic (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchematicSymbol {
    uuid: Uuid,
}

impl SchematicSymbol {
    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> SymbolId {
        SymbolId(self.uuid)
    }
}
