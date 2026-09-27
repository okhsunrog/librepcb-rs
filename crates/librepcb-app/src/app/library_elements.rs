//! Library element tabs on the application side (symbol, package,
//! component, device, category and organization editors, milestone M4b):
//! opening elements from the library tab, new elements and closing the
//! tabs of removed elements.

use librepcb_app_ui as ui;
use librepcb_core::fileio::FilePath;

use super::State;

impl State {
    /// Opens (or duplicates) a library element in its editor tab.
    pub(crate) fn open_library_element_tab(
        &mut self,
        _library: &FilePath,
        kind: ui::LibraryTreeViewItemType,
        _path: &FilePath,
        _duplicate: bool,
    ) {
        self.not_implemented(&format!("{kind:?}"));
    }

    /// Opens the tab of a new library element.
    pub(crate) fn open_new_library_element_tab(
        &mut self,
        _library: &FilePath,
        action: ui::LibraryAction,
    ) {
        self.not_implemented(&format!("{action:?}"));
    }

    /// Closes the tabs of the elements in `paths` (before removing them).
    pub(crate) fn close_library_element_tabs(&mut self, _paths: &[FilePath]) {}
}
