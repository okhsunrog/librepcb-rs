//! Port of libs/librepcb/core/project/board (boards and their items).
//!
//! Wave 3b (board items) fills in the item types, settings and derived data
//! of this module; the [`Board`] container, its header (UUID, name) and its
//! place in the project are complete. Until the items are ported, the
//! loaded `board.lp` and `settings.user.lp` are kept verbatim and written
//! back unchanged ([`Board::raw`](Board)).

mod air_wire;
#[allow(clippy::module_inception)] // Upstream file name.
mod board;
mod change;
mod device;
mod net_segment;
mod plane;

pub use air_wire::AirWire;
pub use board::{Board, BoardDerived, BoardProperties};
pub use change::BoardChange;
pub use device::BoardDevice;
pub use net_segment::BoardNetSegment;
pub use plane::BoardPlane;
