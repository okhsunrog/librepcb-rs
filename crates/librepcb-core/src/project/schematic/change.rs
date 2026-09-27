//! Item level change events of a schematic (upstream: the `Schematic`
//! signals `symbolAdded()`, `netSegmentAdded()`, ... and the items'
//! `onEdited` signals).
//!
//! TODO(wave3b/schematic): add the variants (`SymbolAdded`,
//! `SymbolRemoved`, `SymbolPlacement`, `NetSegmentElements { .. }`, ...).

/// A change of an item of a schematic, see
/// [`Change::Schematic`](crate::project::Change::Schematic).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum SchematicChange {
    /// Placeholder until the items are ported: something in the schematic
    /// changed, rebuild it.
    Rebuild,
}
