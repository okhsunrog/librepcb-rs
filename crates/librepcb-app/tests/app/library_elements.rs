//! Library element editors (M4b) in the headless application: opening
//! elements from the library tab, editing with the FSMs and tool bars,
//! properties dialogs, metadata, checks, undo and saving. Screenshots
//! `element_*.png` are written to `$CARGO_TARGET_TMPDIR`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use librepcb_app::screenshot::{Headless, write_png};
use librepcb_app::tabs::{Tab, TabRequest};
use librepcb_app::{App, startup, ui};
use librepcb_core::fileio::{FilePath, file_utils};
use librepcb_core::types::Point;
use librepcb_editor::fsm::library::LibraryTool;
use slint::{ComponentHandle, Model};

pub const W: u32 = 1400;
pub const H: u32 = 900;

/// The diode symbol of the upstream test library.
pub const DIODE_SYMBOL: &str = "9b75d0ce-ac4e-4a52-a88a-8777f66d3241";

pub fn save(headless: &Headless, name: &str) {
    let pixels = headless.settle(20);
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    write_png(&path, W, H, &pixels).unwrap();
    println!("screenshot: {}", path.display());
}

/// A headless application with a workspace containing a copy of the
/// upstream test library; returns the app and the library directory.
pub fn app_with_library(headless: &Headless, dir: &Path) -> (App, FilePath) {
    let workspace = startup::open_workspace(&dir.join("workspace")).unwrap();
    let lib = workspace
        .local_libraries_path()
        .path_to("Populated Library.lplib");
    file_utils::copy_dir_recursively(
        &FilePath::new(
            Path::new(env!("LIBREPCB_UPSTREAM_DIR"))
                .join("tests/data/libraries/Populated Library.lplib"),
        )
        .unwrap(),
        &lib,
    )
    .unwrap();
    workspace
        .library_db()
        .rescan(&AtomicBool::new(false), &mut |_| {})
        .unwrap();
    librepcb_i18n::set_language("en").unwrap();
    let window = ui::AppWindow::new().unwrap();
    slint::select_bundled_translation("en").unwrap();
    let app = App::new(window, workspace);
    app.window().show().unwrap();
    let data = app.window().global::<ui::Data>();
    headless.run_until(std::time::Duration::from_secs(20), || {
        !data.get_libraries_rescan_in_progress()
    });
    headless.settle(5);
    (app, lib)
}

pub fn find_tab(app: &App, f: impl Fn(&Tab) -> bool) -> Option<(usize, usize)> {
    let state = app.state().borrow();
    state
        .sections()
        .iter()
        .enumerate()
        .find_map(|(si, s)| s.tabs().iter().position(&f).map(|ti| (si, ti)))
}

pub fn with_tab<R>(app: &App, pos: (usize, usize), f: impl FnOnce(&mut Tab) -> R) -> R {
    let mut state = app.state().borrow_mut();
    f(state.sections_mut()[pos.0].tab_mut(pos.1).unwrap())
}

fn mm(x: f64, y: f64) -> Point {
    Point::from_mm(x, y).unwrap()
}

#[test]
fn symbol_editor() {
    crate::common::with_headless(W, H, |headless| {
        let tmp = tempfile::tempdir().unwrap();
        let (app, lib) = app_with_library(headless, tmp.path());
        let backend = app.window().global::<ui::Backend>();
        let forms = app.window().global::<ui::Dialogs>();

        // Open the symbol from the library tab (like double-clicking it).
        backend.invoke_trigger_library(lib.to_native().into(), ui::LibraryAction::Open);
        headless.settle(10);
        let lib_tab = find_tab(&app, |t| matches!(t, Tab::Library(_))).unwrap();
        let sym_dir = lib.path_to(&format!("sym/{DIODE_SYMBOL}"));
        let update = with_tab(&app, lib_tab, |t| {
            let Tab::Library(t) = t else { unreachable!() };
            let rows = t.elements_model();
            let row = (0..rows.len())
                .find(|i| rows.get(*i).unwrap().user_data == sym_dir.as_str())
                .expect("diode symbol row");
            let mut d = t.derived_ui_data();
            d.filtered_elements_index = row as i32;
            t.set_derived_ui_data(&d);
            t.trigger(ui::TabAction::EditProperties)
        });
        assert!(update.requests.iter().any(|r| matches!(
            r,
            TabRequest::OpenLibraryElement { path, .. } if *path == sym_dir
        )));
        backend.invoke_trigger_tab(
            lib_tab.0 as i32,
            lib_tab.1 as i32,
            ui::TabAction::EditProperties,
        );
        headless.settle(10);
        let sym = find_tab(&app, |t| matches!(t, Tab::Symbol(_))).expect("symbol tab");
        let symbol = |app: &App| {
            with_tab(app, sym, |t| {
                let Tab::Symbol(t) = t else { unreachable!() };
                (
                    t.core().editor.element().polygons().len(),
                    t.core().editor.element().pins().len(),
                )
            })
        };
        let (polygons, pins) = symbol(&app);
        assert!(pins >= 2);
        {
            let data = app.window().global::<ui::Data>();
            let sections = data.get_sections();
            let tabs = sections.row_data(sym.0).unwrap().symbol_tabs;
            let d = tabs.row_data(sym.1).unwrap();
            assert_eq!(d.name, "Diode");
            assert_eq!(d.page_index, 1);
            assert!(d.categories.row_count() >= 1, "category with path");
        }
        save(headless, "element_symbol.png");

        // Draw a line with the line tool.
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            t.trigger(ui::TabAction::ToolLine);
            assert_eq!(t.fsm().tool(), LibraryTool::DrawLine);
            assert_eq!(t.derived_ui_data().tool, ui::EditorTool::Line);
            let _ = t.render_scene(1000.0, 800.0, 1.0);
            for p in [mm(5.08, 5.08), mm(10.16, 5.08)] {
                let s = t
                    .canvas()
                    .view()
                    .world_to_screen(librepcb_app::tabs::editing::point_to_world(p));
                use librepcb_canvas::{Modifiers, PointerButton, PointerKind};
                t.pointer_event(
                    PointerKind::Move,
                    PointerButton::Other,
                    s,
                    Modifiers::default(),
                );
                t.pointer_event(
                    PointerKind::Down,
                    PointerButton::Left,
                    s,
                    Modifiers::default(),
                );
                t.pointer_event(
                    PointerKind::Up,
                    PointerButton::Left,
                    s,
                    Modifiers::default(),
                );
                std::thread::sleep(std::time::Duration::from_millis(510));
            }
            t.trigger(ui::TabAction::Abort);
            t.trigger(ui::TabAction::Abort);
        });
        assert_eq!(symbol(&app).0, polygons + 1);
        save(headless, "element_symbol_line.png");
        // Undo removes it again.
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            assert!(t.ui_data().unsaved_changes);
            t.trigger(ui::TabAction::Undo);
        });
        assert_eq!(symbol(&app).0, polygons);

        // Pin properties dialog: rename a pin.
        let pin = with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            *t.core().editor.element().pins().uuids().first().unwrap()
        });
        let id = {
            let state = app.state().borrow();
            state.sections()[sym.0].tabs()[sym.1].id()
        };
        app.state().borrow_mut().open_library_item_properties(
            id,
            librepcb_app::tabs::LibraryItemRef::Symbol(
                librepcb_editor::library_editor::commands::SymbolItem::Pin(pin),
            ),
        );
        headless.settle(10);
        assert!(forms.get_form_shown());
        save(headless, "element_symbol_pin_dialog.png");
        {
            let mut state = app.state().borrow_mut();
            let dialog = state.form_dialog().unwrap();
            dialog
                .dialog
                .form_mut()
                .edit("name", |f| f.text = "XYZ".into());
        }
        forms.invoke_form_button(-1);
        headless.settle(10);
        assert!(!forms.get_form_shown());
        let name = with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            t.core()
                .editor
                .element()
                .pins()
                .by_uuid(&pin)
                .unwrap()
                .name()
                .to_string()
        });
        assert_eq!(name, "XYZ");

        // Metadata: description, then save; the file is written.
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            let mut d = t.derived_ui_data();
            d.description = "Edited description".into();
            t.set_derived_ui_data(&d);
            t.trigger(ui::TabAction::Save);
            assert!(!t.ui_data().unsaved_changes);
        });
        headless.settle(10);
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            assert!(!t.ui_data().unsaved_changes, "still saved after UI updates");
        });
        let content = std::fs::read_to_string(sym_dir.path_to("symbol.lp").as_path()).unwrap();
        assert!(content.contains("Edited description"));
        assert!(content.contains("(name \"XYZ\")"));

        // Checks with approvals.
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            let checks = t.derived_ui_data().checks;
            assert!(checks.execution_error.is_empty());
        });

        // A new symbol (wizard mode), saved into the library.
        backend.invoke_trigger_library(lib.to_native().into(), ui::LibraryAction::NewSymbol);
        headless.settle(10);
        let new = find_tab(
            &app,
            |t| matches!(t, Tab::Symbol(s) if s.core().wizard_mode),
        )
        .expect("new symbol tab");
        save(headless, "element_symbol_new.png");
        with_tab(&app, sym, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            let d = t.core().editor.is_dirty();
            let ui = t.ui_data().unsaved_changes;
            assert!(!ui && !d, "diode still saved: {ui} {d}");
        });
        let new_dir = with_tab(&app, new, |t| {
            let Tab::Symbol(t) = t else { unreachable!() };
            let mut d = t.derived_ui_data();
            assert_eq!(d.name, "");
            d.name = "My Symbol".into();
            t.set_derived_ui_data(&d);
            t.trigger(ui::TabAction::Save);
            assert!(!t.core().wizard_mode);
            t.core().directory_path()
        });
        assert!(new_dir.path_to("symbol.lp").is_existing_file());
    });
}
