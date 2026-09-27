//! Library element tabs on the application side (symbol, package,
//! component, device, category and organization editors, milestone M4b):
//! opening elements from the library tab, new and duplicated elements,
//! the requests of the element tabs and closing the tabs of removed
//! elements.
//!
//! Port of the element tab parts of libs/librepcb/editor/mainwindow.cpp
//! (`openSymbolTab()`, `openNewSymbolTab()`, ...).

use std::rc::Rc;

use librepcb_app_ui as ui;
use librepcb_core::fileio::FilePath;
use librepcb_core::library::sym::Symbol;
use librepcb_i18n::tr;
use slint::ComponentHandle;

use super::{State, deferred};
use crate::open_library::OpenLibrary;
use crate::tabs::element_core::{ElementCore, ElementKindInfo, OpenMode};
use crate::tabs::{LibraryItemRef, SymbolTab, Tab, TabId, TabUpdate};

impl State {
    /// Opens (or duplicates) a library element in its editor tab.
    pub(crate) fn open_library_element_tab(
        &mut self,
        library: &FilePath,
        kind: ui::LibraryTreeViewItemType,
        path: &FilePath,
        duplicate: bool,
    ) {
        if !duplicate
            && let Some((sec, index)) =
                self.find_tab_where(|t| t.directory_path().as_ref() == Some(path))
        {
            self.set_current_tab(sec, index as i32, true);
            return;
        }
        let Some(lib) = self.open_library(library) else {
            return;
        };
        let Some(relative) = lib.relative_path(path) else {
            log::warn!(
                "Element {} not in library {}",
                path.to_native(),
                library.to_native()
            );
            return;
        };
        let mode = if duplicate {
            OpenMode::Duplicate
        } else {
            OpenMode::Open
        };
        self.open_element_tab(&lib, kind, Some(&relative), mode);
    }

    /// Opens the tab of a new library element (upstream
    /// `openNewSymbolTab()` etc.).
    pub(crate) fn open_new_library_element_tab(
        &mut self,
        library: &FilePath,
        action: ui::LibraryAction,
    ) {
        let kind = match action {
            ui::LibraryAction::NewSymbol => ui::LibraryTreeViewItemType::Symbol,
            ui::LibraryAction::NewPackage => ui::LibraryTreeViewItemType::Package,
            ui::LibraryAction::NewComponent => ui::LibraryTreeViewItemType::Component,
            ui::LibraryAction::NewDevice => ui::LibraryTreeViewItemType::Device,
            ui::LibraryAction::NewComponentCategory => {
                ui::LibraryTreeViewItemType::ComponentCategory
            }
            ui::LibraryAction::NewPackageCategory => ui::LibraryTreeViewItemType::PackageCategory,
            ui::LibraryAction::NewOrganization => ui::LibraryTreeViewItemType::Organization,
            _ => return,
        };
        let Some(lib) = self.open_library(library) else {
            return;
        };
        self.open_element_tab(&lib, kind, None, OpenMode::New);
    }

    /// Creates the element core of a tab with its model handlers.
    fn element_core<E: ElementKindInfo>(
        &mut self,
        id: TabId,
        lib: &Rc<OpenLibrary>,
        relative: Option<&str>,
        mode: OpenMode,
    ) -> Option<ElementCore<E>> {
        let (db, locales, user) = {
            let ws = self.workspace.lock();
            (
                ws.shared_library_db(),
                ws.settings().library_locale_order.get().clone(),
                ws.settings().user_name.get().clone(),
            )
        };
        let w = self.this.clone();
        let on_check = move |row, data| {
            deferred(&w, move |s| {
                s.element_tab_event(id, |t| t.element_check_row(row, &data));
            });
        };
        let w = self.this.clone();
        let on_category = move |row, data| {
            deferred(&w, move |s| {
                s.element_tab_event(id, |t| t.element_category_row(row, &data));
            });
        };
        match ElementCore::<E>::open(
            Rc::clone(lib),
            relative,
            mode,
            db,
            locales,
            user,
            on_check,
            on_category,
        ) {
            Ok(core) => Some(core),
            Err(e) => {
                self.notifications
                    .borrow_mut()
                    .push(crate::notifications::Notification {
                        auto_popup: true,
                        ..crate::notifications::Notification::new(
                            ui::NotificationType::Critical,
                            tr!("MainWindow", "Error"),
                            e,
                        )
                    });
                None
            }
        }
    }

    /// Opens an element tab of a kind.
    fn open_element_tab(
        &mut self,
        lib: &Rc<OpenLibrary>,
        kind: ui::LibraryTreeViewItemType,
        relative: Option<&str>,
        mode: OpenMode,
    ) {
        let id = TabId::new();
        let library_index = self
            .libraries
            .iter()
            .position(|l| Rc::ptr_eq(l, lib))
            .map_or(-1, |i| i as i32);
        let mut tab = match kind {
            ui::LibraryTreeViewItemType::Symbol => {
                let Some(core) = self.element_core::<Symbol>(id, lib, relative, mode) else {
                    return;
                };
                let style = *self.workspace.lock().settings().schematic_grid_style.get();
                Tab::Symbol(Box::new(SymbolTab::new(core, style).with_id(id)))
            }
            other => {
                self.not_implemented(&format!("{other:?}"));
                return;
            }
        };
        tab.set_library_index(library_index);
        self.add_tab(tab);
        if let Some(w) = self.window() {
            let d = w.global::<ui::Data>();
            d.invoke_set_current_library(library_index);
            d.invoke_set_panel_page(ui::PanelPage::Documents);
        }
    }

    /// Runs `f` on the tab `id` and applies its update.
    pub(crate) fn element_tab_event(&mut self, id: TabId, f: impl FnOnce(&mut Tab) -> TabUpdate) {
        let Some((si, ti)) = self.find_tab(id) else {
            return;
        };
        let update = match self.sections[si].tab_mut(ti) {
            Some(t) => f(t),
            None => return,
        };
        self.apply_update(si, ti, update);
    }

    /// Opens a copy of the element of tab `id` as new element (upstream
    /// "save as" of the element tabs).
    pub(crate) fn duplicate_element_tab(&mut self, id: TabId) {
        let Some((si, ti)) = self.find_tab(id) else {
            return;
        };
        let (lib, kind, path) = match &mut self.sections[si].tabs_mut()[ti] {
            Tab::Symbol(t) => {
                t.core_mut().element_duplicated = true;
                (
                    Rc::clone(&t.core().library),
                    ui::LibraryTreeViewItemType::Symbol,
                    t.core().directory_path(),
                )
            }
            _ => return,
        };
        self.sections[si].refresh_tab(ti, &self.projects);
        let Some(relative) = lib.relative_path(&path) else {
            return;
        };
        if path.is_existing_dir() {
            self.open_element_tab(&lib, kind, Some(&relative), OpenMode::Duplicate);
        }
    }

    /// Opens the properties dialog of an item of a library element tab
    /// (upstream: the element FSMs open the dialogs).
    pub fn open_library_item_properties(&mut self, tab: TabId, item: LibraryItemRef) {
        use crate::dialogs::library_items::{
            LibraryCircleDialog, LibraryPolygonDialog, ObjectOwner, PinDialog, SymbolTextDialog,
        };
        use librepcb_editor::fsm::library::{ElementHost, SymbolHost};
        use librepcb_editor::library_editor::commands::SymbolObject;
        let Some((si, ti)) = self.find_tab(tab) else {
            return;
        };
        let dialog: Option<Box<dyn crate::dialogs::FormDialog>> =
            match (&self.sections[si].tabs()[ti], item) {
                (Tab::Symbol(t), LibraryItemRef::Symbol(item)) => {
                    let unit = t.length_unit();
                    match SymbolObject::from_symbol(t.core().editor.element(), item) {
                        Some(SymbolObject::Pin(pin)) => Some(Box::new(PinDialog::new(pin, unit))),
                        Some(SymbolObject::Polygon(p)) => {
                            Some(Box::new(LibraryPolygonDialog::new(
                                ObjectOwner::Symbol,
                                p,
                                &SymbolHost::polygon_layers(),
                                unit,
                            )))
                        }
                        Some(SymbolObject::Circle(c)) => Some(Box::new(LibraryCircleDialog::new(
                            ObjectOwner::Symbol,
                            c,
                            &SymbolHost::polygon_layers(),
                            unit,
                        ))),
                        Some(SymbolObject::Text(text)) => Some(Box::new(SymbolTextDialog::new(
                            text,
                            &SymbolHost::text_layers(),
                            unit,
                        ))),
                        _ => None,
                    }
                }
                _ => self.library_item_dialog(si, ti, item),
            };
        if let Some(dialog) = dialog {
            self.open_library_dialog(Some(tab), dialog);
        }
    }

    /// The properties dialogs of package items (see the package tab).
    fn library_item_dialog(
        &self,
        _si: usize,
        _ti: usize,
        _item: LibraryItemRef,
    ) -> Option<Box<dyn crate::dialogs::FormDialog>> {
        None
    }

    /// Opens the "import pins" dialog of a symbol tab.
    pub(crate) fn open_import_pins_dialog(&mut self, tab: TabId) {
        let dialog = crate::dialogs::library_items::ImportPinsDialog::new();
        self.open_library_dialog(Some(tab), Box::new(dialog));
    }

    /// Closes the tabs of the elements in `paths` (before removing them).
    pub(crate) fn close_library_element_tabs(&mut self, paths: &[FilePath]) {
        for si in 0..self.sections.len() {
            while let Some(ti) = self.sections[si].tabs().iter().position(|t| {
                !matches!(t, Tab::Library(_))
                    && t.directory_path().is_some_and(|p| paths.contains(&p))
            }) {
                self.sections[si].remove_tab(ti);
            }
            self.update_section_row(si);
        }
    }
}
