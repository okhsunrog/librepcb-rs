//! Item level change events of a board (upstream: the `Board` signals
//! `deviceAdded()`, `netSegmentAdded()`, `planeAdded()`, `airWireAdded()`,
//! ... and the items' `onEdited` signals).
//!
//! TODO(wave3b/board): add the variants (`DeviceAdded`, `DevicePlacement`,
//! `NetSegmentElements { .. }`, `Plane { .. }`, `AirWires`,
//! `PlaneFragments`, ...).

/// A change of an item of a board, see
/// [`Change::Board`](crate::project::Change::Board).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BoardChange {
    /// Placeholder until the items are ported: something in the board
    /// changed, rebuild it.
    Rebuild,
}
