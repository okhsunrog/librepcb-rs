//! Editing in the schematic and board tabs through the Rust API of the
//! tabs, with simulated pointer events in screen coordinates (like the
//! Slint UI sends them): moving a symbol, drawing a wire, the context menu,
//! copy/paste, drawing a trace and undo.
//!
//! The tabs work without a Slint window (the scene is rendered into an
//! image), so no platform is needed.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use librepcb_app::project::AppProject;
use librepcb_app::tabs::editing::point_to_world;
use librepcb_app::tabs::{Board2dTab, SchematicTab, TabRequest, TabUpdate};
use librepcb_app::ui;
use librepcb_canvas::kurbo::Point as ScreenPoint;
use librepcb_canvas::{Modifiers, PointerButton, PointerKind};
use librepcb_core::fileio::{FilePath, TransactionalDirectory, TransactionalFileSystem};
use librepcb_core::project::{BoardId, SchematicId, SymbolId};
use librepcb_core::types::{Angle, CircuitIdentifier, ElementName, GridStyle, Length, Point, Uuid};
use librepcb_editor::commands::{
    AddBoard, AddComponent, AddDevice, AddSchematic, ConnectPinToNet, PinRef, SetBoardOutline,
    SymbolPlacement,
};
use librepcb_editor::fsm::board::BoardTool;
use librepcb_editor::fsm::schematic::SchematicTool;
use librepcb_editor::{DirectoryLibrarySource, ProjectEditor};

fn mm(x: f64, y: f64) -> Point {
    Point::new(Length::from_mm(x).unwrap(), Length::from_mm(y).unwrap())
}

fn uuid(s: &str) -> Uuid {
    s.parse().unwrap()
}

const RESISTOR: &str = "ef80cd5e-2689-47ee-8888-31d04fc99174";
const R0805: &str = "078650d3-483c-4b9e-a848-b14f1aad2edc";

fn library_source() -> Arc<DirectoryLibrarySource> {
    let dir = FilePath::new(
        Path::new(env!("LIBREPCB_UPSTREAM_DIR"))
            .join("tests/data/libraries/Populated Library.lplib"),
    )
    .unwrap();
    Arc::new(DirectoryLibrarySource::from_libraries([&dir]).unwrap())
}

/// Creates a project with the resistors R1 and R2 on a schematic and a
/// board (not connected), saves it and opens it like the application.
fn create_project(dir: &Path) -> (Rc<AppProject>, SchematicId, BoardId) {
    {
        let fs = TransactionalFileSystem::open_rw(&FilePath::new(dir).unwrap()).unwrap();
        let directory = TransactionalDirectory::new(Arc::new(fs), "");
        let fonts = FilePath::new(
            Path::new(env!("LIBREPCB_UPSTREAM_DIR")).join("share/librepcb/fontobene"),
        )
        .unwrap();
        let project = librepcb_editor::create_project(directory, "test.lpp", Some(&fonts)).unwrap();
        let mut editor = ProjectEditor::with_source(project, library_source());
        let schematic = editor
            .execute(AddSchematic {
                name: ElementName::new("Main").unwrap(),
                index: None,
            })
            .unwrap();
        let board = editor
            .execute(AddBoard::new(ElementName::new("default").unwrap()))
            .unwrap()
            .board;
        for (name, x) in [("R1", 20.32), ("R2", 40.64)] {
            editor
                .execute(AddComponent {
                    device: Some(uuid(R0805)),
                    place: Some(SymbolPlacement {
                        schematic: Some(schematic),
                        position: mm(x, 50.8),
                        rotation: Angle::DEG0,
                        mirrored: false,
                        gate_offset: None,
                    }),
                    ..AddComponent::new(uuid(RESISTOR))
                })
                .unwrap();
            editor
                .execute(AddDevice {
                    component: name.into(),
                    board: Some(board),
                    device: None,
                    footprint: None,
                    position: mm(x / 2.0, 10.0),
                    rotation: Angle::DEG0,
                    mirrored: false,
                })
                .unwrap();
        }
        // R1.1 and R2.2 on the net GND (air wire on the board).
        for (component, pin) in [("R1", "1"), ("R2", "2")] {
            editor
                .execute(ConnectPinToNet {
                    pin: PinRef::new(component, pin),
                    net: CircuitIdentifier::new("GND").unwrap(),
                    label: true,
                    stub_length: None,
                })
                .unwrap();
        }
        editor
            .execute(SetBoardOutline::rect(
                Some(board),
                mm(0.0, 0.0),
                mm(40.0, 20.0),
            ))
            .unwrap();
        editor.save().unwrap();
    }
    let lpp = FilePath::new(dir.join("test.lpp")).unwrap();
    let project = Rc::new(AppProject::open(&lpp, library_source()).unwrap());
    assert!(project.is_writable());
    let (sch, brd) = {
        let p = project.shared().lock();
        (
            p.project().schematics()[0].id(),
            p.project().boards()[0].id(),
        )
    };
    (project, sch, brd)
}

/// Simulates pointer input on a tab.
trait Input {
    fn pointer(&mut self, kind: PointerKind, button: PointerButton, pos: ScreenPoint) -> TabUpdate;
    fn screen(&self, p: Point) -> ScreenPoint;

    fn move_to(&mut self, p: Point) -> TabUpdate {
        let s = self.screen(p);
        self.pointer(PointerKind::Move, PointerButton::Other, s)
    }

    fn press(&mut self, p: Point) -> TabUpdate {
        let s = self.screen(p);
        self.pointer(PointerKind::Down, PointerButton::Left, s)
    }

    fn release(&mut self, p: Point) -> TabUpdate {
        let s = self.screen(p);
        self.pointer(PointerKind::Up, PointerButton::Left, s)
    }

    fn click(&mut self, p: Point) {
        self.move_to(p);
        self.press(p);
        self.release(p);
        // Avoid double clicks between consecutive clicks at the same place.
        std::thread::sleep(std::time::Duration::from_millis(510));
    }

    fn right_click(&mut self, p: Point) -> TabUpdate {
        let s = self.screen(p);
        self.move_to(p);
        self.pointer(PointerKind::Down, PointerButton::Right, s);
        self.pointer(PointerKind::Up, PointerButton::Right, s)
    }
}

impl Input for SchematicTab {
    fn pointer(&mut self, kind: PointerKind, button: PointerButton, pos: ScreenPoint) -> TabUpdate {
        self.pointer_event(kind, button, pos, Modifiers::default())
    }

    fn screen(&self, p: Point) -> ScreenPoint {
        self.canvas().view().world_to_screen(point_to_world(p))
    }
}

impl Input for Board2dTab {
    fn pointer(&mut self, kind: PointerKind, button: PointerButton, pos: ScreenPoint) -> TabUpdate {
        self.pointer_event(kind, button, pos, Modifiers::default())
    }

    fn screen(&self, p: Point) -> ScreenPoint {
        self.canvas().view().world_to_screen(point_to_world(p))
    }
}

fn symbol(project: &AppProject, sch: SchematicId, name: &str) -> (SymbolId, Point) {
    let p = project.shared().lock();
    let proj = p.project();
    let (c, _) = proj.circuit().component_instance_by_name(name).unwrap();
    let s = proj.schematic(sch).unwrap();
    let (id, sym) = s
        .symbols()
        .iter()
        .find(|(_, s)| s.component() == c)
        .unwrap();
    (*id, sym.position())
}

/// Pin positions of a symbol, sorted by X.
fn pins(project: &AppProject, sch: SchematicId, name: &str) -> Vec<Point> {
    let (id, _) = symbol(project, sch, name);
    let p = project.shared().lock();
    let proj = p.project();
    let s = proj.schematic(sch).unwrap();
    let mut pins: Vec<Point> = s.symbols()[&id]
        .pins(proj.view())
        .unwrap()
        .into_iter()
        .map(|v| v.position())
        .collect();
    pins.sort_by_key(|p| p.x);
    pins
}

fn wire_count(project: &AppProject, sch: SchematicId) -> usize {
    let p = project.shared().lock();
    p.project()
        .schematic(sch)
        .unwrap()
        .net_segments()
        .values()
        .map(|s| s.lines().len())
        .sum()
}

fn trace_count(project: &AppProject, board: BoardId) -> usize {
    let p = project.shared().lock();
    p.project()
        .board(board)
        .unwrap()
        .net_segments()
        .values()
        .map(|s| s.traces().len())
        .sum()
}

fn undo_index(project: &AppProject) -> usize {
    project.shared().lock().editor.undo_stack().index()
}

#[test]
fn schematic_move_symbol_draw_wire_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (project, sch, _) = create_project(dir.path());
    let mut tab = SchematicTab::new(Rc::clone(&project), sch, GridStyle::Lines);
    let _ = tab.render_scene(1200.0, 800.0, 1.0);

    // Hovering a symbol shows the pointing hand cursor or info box.
    let (r2, r2_pos) = symbol(&project, sch, "R2");
    let wires_before = wire_count(&project, sch);

    // Drag R2 by its origin.
    let index = undo_index(&project);
    tab.move_to(r2_pos);
    tab.press(r2_pos);
    let target = r2_pos + mm(5.0, 10.0);
    tab.move_to(target);
    assert!(
        project
            .shared()
            .lock()
            .editor
            .undo_stack()
            .is_group_active()
    );
    let update = tab.release(target);
    assert!(update.repaint && update.data_changed);
    assert!(
        !project
            .shared()
            .lock()
            .editor
            .undo_stack()
            .is_group_active()
    );
    assert_eq!(undo_index(&project), index + 1);
    let (_, moved) = symbol(&project, sch, "R2");
    assert_eq!(moved, r2_pos + mm(5.08, 10.16));
    // The moved symbol is selected and highlighted in the scene.
    assert!(
        tab.fsm()
            .selection()
            .contains(&librepcb_editor::fsm::schematic::SchematicItem::Symbol(r2))
    );
    assert!(tab.scene().unwrap().scene().selection().next().is_some());

    // Draw a wire from R1 pin 2 to R2 pin 1.
    let from = pins(&project, sch, "R1")[1];
    let to = pins(&project, sch, "R2")[0];
    tab.trigger(ui::TabAction::ToolWire);
    assert_eq!(tab.fsm().tool(), SchematicTool::Wire);
    assert_eq!(tab.derived_ui_data(&[]).tool, ui::EditorTool::Wire);
    tab.click(from);
    tab.move_to(to);
    // The preview is in the model and the scene.
    assert!(wire_count(&project, sch) > wires_before);
    tab.click(to);
    assert!(
        !project
            .shared()
            .lock()
            .editor
            .undo_stack()
            .is_group_active()
    );
    let wires = wire_count(&project, sch);
    assert!(wires > wires_before);
    // Escape (the abort shortcut) leaves the tool.
    tab.trigger(ui::TabAction::Abort);
    assert_eq!(tab.fsm().tool(), SchematicTool::Select);

    // Undo the wire (and its simplification), then the move.
    while wire_count(&project, sch) > wires_before {
        tab.trigger(ui::TabAction::Undo);
    }
    tab.trigger(ui::TabAction::Undo);
    assert_eq!(symbol(&project, sch, "R2").1, r2_pos);
    assert_eq!(undo_index(&project), index);
    // Redo is available.
    assert_eq!(tab.ui_data().features.redo, ui::FeatureState::Enabled);
    tab.trigger(ui::TabAction::Redo);
    assert_eq!(symbol(&project, sch, "R2").1, moved);
}

#[test]
fn schematic_context_menu_and_clipboard() {
    let dir = tempfile::tempdir().unwrap();
    let (project, sch, _) = create_project(dir.path());
    let mut tab = SchematicTab::new(Rc::clone(&project), sch, GridStyle::Lines);
    let _ = tab.render_scene(1200.0, 800.0, 1.0);
    let (r1, r1_pos) = symbol(&project, sch, "R1");

    // Right click: the context menu of the symbol.
    let update = tab.right_click(r1_pos);
    let entries = update
        .requests
        .iter()
        .find_map(|r| match r {
            TabRequest::ContextMenu { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .expect("context menu");
    let rotate = entries
        .iter()
        .position(|e| e.text == "Rotate Counterclockwise")
        .expect("rotate entry");
    tab.context_menu_action(rotate);
    {
        let p = project.shared().lock();
        let s = p.project().schematic(sch).unwrap();
        assert_eq!(s.symbols()[&r1].rotation(), Angle::DEG90);
    }
    // Properties: the dialog is not available yet (notification).
    let update = tab.trigger(ui::TabAction::EditProperties);
    assert!(
        update
            .requests
            .iter()
            .any(|r| matches!(r, TabRequest::Notify(_)))
    );

    // Copy the selected symbol and paste it somewhere else.
    let symbols_before = project
        .shared()
        .lock()
        .project()
        .schematic(sch)
        .unwrap()
        .symbols()
        .len();
    assert!(tab.fsm().view_state().features.copy);
    tab.trigger(ui::TabAction::Copy);
    let target = r1_pos + mm(0.0, 20.32);
    tab.move_to(target);
    tab.trigger(ui::TabAction::Paste);
    tab.move_to(target);
    tab.click(target);
    let symbols = project
        .shared()
        .lock()
        .project()
        .schematic(sch)
        .unwrap()
        .symbols()
        .len();
    assert_eq!(symbols, symbols_before + 1);

    // Select all and delete.
    tab.trigger(ui::TabAction::SelectAll);
    tab.trigger(ui::TabAction::Delete);
    let symbols = project
        .shared()
        .lock()
        .project()
        .schematic(sch)
        .unwrap()
        .symbols()
        .len();
    assert_eq!(symbols, 0);
    tab.trigger(ui::TabAction::Undo);
    let symbols = project
        .shared()
        .lock()
        .project()
        .schematic(sch)
        .unwrap()
        .symbols()
        .len();
    assert_eq!(symbols, symbols_before + 1);
}

#[test]
fn board_draw_trace_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _, board) = create_project(dir.path());
    let mut tab = Board2dTab::new(Rc::clone(&project), board, GridStyle::Lines);
    let _ = tab.render_scene(1200.0, 800.0, 1.0);

    // Pads of the GND net: R1.1 and R2.2.
    let pad = |name: &str, pad: &str| {
        let p = project.shared().lock();
        let proj = p.project();
        let (c, _) = proj.circuit().component_instance_by_name(name).unwrap();
        proj.board(board)
            .unwrap()
            .device(c)
            .unwrap()
            .pads(proj.library(), proj.circuit())
            .unwrap()
            .into_iter()
            .find(|v| v.package_pad().is_some_and(|pp| pp.name().as_str() == pad))
            .unwrap()
            .position()
    };
    let a = pad("R1", "1");
    let b = pad("R2", "2");
    let index = undo_index(&project);

    tab.trigger(ui::TabAction::ToolWire);
    assert_eq!(tab.fsm().tool(), BoardTool::DrawTrace);
    let data = tab.derived_ui_data(&[]);
    assert_eq!(data.tool, ui::EditorTool::Wire);
    assert!(data.tool_layer.current_index >= 0);
    tab.click(a);
    // The net of the trace is highlighted and cross-probed.
    assert!(!tab.fsm().highlighted_nets().is_empty());
    assert!(!tab.cross_probe().nets.is_empty());
    tab.move_to(b);
    tab.click(b);
    assert!(
        !project
            .shared()
            .lock()
            .editor
            .undo_stack()
            .is_group_active()
    );
    assert!(trace_count(&project, board) > 0);
    assert_eq!(undo_index(&project), index + 1);

    // Tool bar: change the trace width through the UI data.
    let mut data = tab.derived_ui_data(&[]);
    data.tool_line_width.increase = true;
    let before = tab.fsm().tool_data().line_width;
    tab.set_derived_ui_data(&data);
    assert_ne!(tab.fsm().tool_data().line_width, before);

    tab.trigger(ui::TabAction::Abort);
    assert_eq!(tab.fsm().tool(), BoardTool::Select);
    tab.trigger(ui::TabAction::Undo);
    assert_eq!(trace_count(&project, board), 0);
    assert_eq!(undo_index(&project), index);
}

#[test]
fn board_move_device_and_rotate() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _, board) = create_project(dir.path());
    let mut tab = Board2dTab::new(Rc::clone(&project), board, GridStyle::Lines);
    let _ = tab.render_scene(1200.0, 800.0, 1.0);
    let device = |project: &AppProject| {
        let p = project.shared().lock();
        let proj = p.project();
        let (c, _) = proj.circuit().component_instance_by_name("R2").unwrap();
        let d = proj.board(board).unwrap().device(c).unwrap();
        (d.transform().position, d.transform().rotation)
    };
    let (pos, _) = device(&project);
    // Grab the device at a pad.
    let grab = {
        let p = project.shared().lock();
        let proj = p.project();
        let (c, _) = proj.circuit().component_instance_by_name("R2").unwrap();
        let d = proj.board(board).unwrap().device(c).unwrap();
        d.pads(proj.library(), proj.circuit()).unwrap()[0].position()
    };
    tab.move_to(grab);
    tab.press(grab);
    let target = grab + mm(2.0, 3.0);
    tab.move_to(target);
    tab.release(target);
    let (moved, _) = device(&project);
    assert_ne!(moved, pos);
    tab.trigger(ui::TabAction::RotateCcw);
    assert_eq!(device(&project).1, Angle::DEG90);
    tab.trigger(ui::TabAction::Undo);
    tab.trigger(ui::TabAction::Undo);
    assert_eq!(device(&project), (pos, Angle::DEG0));
}
