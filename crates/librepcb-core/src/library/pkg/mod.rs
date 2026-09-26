//! Port of libs/librepcb/core/library/pkg (packages: pads, 3D models and
//! footprints).
//!
//! Like the [`geometry`](crate::geometry) objects, all types are plain data;
//! upstream `onEdited` signals are marked with `// upstream: emits ...`
//! notes. [`FootprintPad`] embeds a [`Pad`](crate::geometry::Pad) (upstream
//! derives from it), accessible with [`FootprintPad::pad()`] and
//! [`FootprintPad::pad_mut()`].
//!
//! Not ported yet: `Package` and `PackageCheck` (they depend on the library
//! element base classes). Not ported (UI specific, to be implemented in the
//! rendering layer): `FootprintPainter`.

mod footprint;
mod footprint_pad;
mod package_model;
mod package_pad;

pub use footprint::{Footprint, FootprintList, FootprintListTag};
pub use footprint_pad::{FootprintPad, FootprintPadList, FootprintPadListTag};
pub use package_model::{PackageModel, PackageModelList, PackageModelListTag};
pub use package_pad::{PackagePad, PackagePadList, PackagePadListTag};
