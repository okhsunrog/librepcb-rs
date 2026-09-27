//! Port of libs/librepcb/core/project/schematic (schematic pages and their
//! items).
//!
//! Wave 3b (schematic items) fills in the item types of this module; the
//! [`Schematic`] container, its header (UUID, name, grid) and its place in
//! the project are complete. Until the items are ported, the loaded
//! `schematic.lp` is kept verbatim and written back unchanged
//! ([`Schematic::raw`](Schematic)).

mod bus_segment;
mod change;
mod net_segment;
#[allow(clippy::module_inception)] // Upstream file name.
mod schematic;
mod symbol;

pub use bus_segment::SchematicBusSegment;
pub use change::SchematicChange;
pub use net_segment::SchematicNetSegment;
pub use schematic::{Schematic, SchematicProperties};
pub use symbol::SchematicSymbol;
