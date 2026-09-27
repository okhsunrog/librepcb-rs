//! Port of libs/librepcb/core/project/board/items/bi_device.{h,cpp} (and
//! `bi_pad.{h,cpp}` for footprint pads, which become computed views).
//!
//! TODO(wave3b/board): placeholder. The final type stores the component
//! instance, library device/footprint/model, position, rotation, mirror,
//! lock and glue flags, attributes and stroke texts; footprint pads are
//! views computed from the library footprint and the device's pad-signal
//! map.

use crate::project::id::ComponentInstanceId;

/// A device placed on a board (placeholder, see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardDevice {
    component: ComponentInstanceId,
}

impl BoardDevice {
    /// Returns the component instance the device belongs to (upstream
    /// `getComponentInstanceUuid()`).
    pub fn component(&self) -> ComponentInstanceId {
        self.component
    }
}
