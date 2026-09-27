//! Tests of the agent oriented commands: net-level wiring
//! (`ConnectNet`, `DisconnectSignals`) and automatic placement
//! (`AutoPlaceSymbols`, `AutoPlaceDevices`).

use librepcb_core::project::erc::run_erc;
use librepcb_core::project::{ComponentSignalRef, Project};
use librepcb_core::rule_check::Severity;
use librepcb_core::types::{CircuitIdentifier, ElementName, Point, Uuid};
use librepcb_editor::ProjectEditor;
use librepcb_editor::commands::*;

use crate::helpers::{create_editor, lib, mm};

fn signal(p: &Project, component: &str, name: &str) -> ComponentSignalRef {
    let (id, c) = p.circuit().component_instance_by_name(component).unwrap();
    let lib = p.library().component(&c.lib_component()).unwrap();
    ComponentSignalRef {
        component: id,
        signal: lib.signals().by_name(name, true).unwrap().uuid(),
    }
}

fn net_of(p: &Project, s: ComponentSignalRef) -> Option<String> {
    let net = p
        .circuit()
        .component_instance(s.component)?
        .signal(&s.signal)?
        .net()?;
    Some(p.circuit().net_signal(net)?.name().to_string())
}

fn add(editor: &mut ProjectEditor, component: Uuid, device: Option<Uuid>) -> String {
    let added = editor
        .execute(AddComponent {
            device,
            place: Some(SymbolPlacement {
                schematic: None,
                position: Point::ORIGIN,
                rotation: Default::default(),
                mirrored: false,
                gate_offset: None,
            }),
            ..AddComponent::new(component)
        })
        .unwrap();
    editor
        .execute(AutoPlaceSymbols {
            symbols: added.symbols,
            margin: None,
        })
        .unwrap();
    added.name
}

#[test]
fn test_connect_net_disconnect_and_auto_place() {
    let tmp = tempfile::tempdir().unwrap();
    let mut editor = create_editor(tmp.path());
    editor
        .execute(AddSchematic {
            name: ElementName::new("Main").unwrap(),
            index: None,
        })
        .unwrap();
    let r1 = add(&mut editor, lib::resistor(), Some(lib::r0805()));
    let c1 = add(&mut editor, lib::capacitor(), Some(lib::c0805()));
    let c2 = add(&mut editor, lib::capacitor(), Some(lib::c0805()));
    let vcc = add(&mut editor, lib::supply_vcc(), None);
    let gnd = add(&mut editor, lib::supply_gnd(), None);

    // Automatic placement: no two symbols at the same place.
    let positions: Vec<Point> = editor.project().schematics()[0]
        .symbols()
        .values()
        .map(|s| s.position())
        .collect();
    for (i, a) in positions.iter().enumerate() {
        for b in &positions[i + 1..] {
            assert_ne!(a, b);
        }
    }

    let p = editor.project();
    let connect = |editor: &mut ProjectEditor, net: Option<&str>, pins: Vec<ComponentSignalRef>| {
        editor
            .execute(ConnectNet {
                net: net.map(|n| CircuitIdentifier::new(n).unwrap()),
                signals: pins,
                max_wire_length: None,
            })
            .unwrap()
    };
    let (r1_1, r1_2) = (signal(p, &r1, "1"), signal(p, &r1, "2"));
    let (c1_1, c1_2) = (signal(p, &c1, "1"), signal(p, &c1, "2"));
    let (c2_1, c2_2) = (signal(p, &c2, "1"), signal(p, &c2, "2"));
    let (vcc_net, gnd_net) = (signal(p, &vcc, "Net"), signal(p, &gnd, "Net"));

    // The forced net name wins over the requested one.
    let r = connect(&mut editor, Some("POWER"), vec![r1_1, c2_1, vcc_net]);
    assert_eq!(r.name, "VCC");
    assert_eq!(r.warnings.len(), 1);
    let r = connect(&mut editor, Some("OUT"), vec![r1_2, c1_1]);
    assert_eq!(r.name, "OUT");
    let r = connect(&mut editor, None, vec![c1_2, c2_2, gnd_net]);
    assert_eq!(r.name, "GND");
    let p = editor.project();
    assert_eq!(net_of(p, c1_2).as_deref(), Some("GND"));
    assert_eq!(p.circuit().net_signals().len(), 3);
    let erc = run_erc(p);
    assert!(
        erc.iter().all(|m| m.message().severity() == Severity::Hint),
        "{:?}",
        erc.iter()
            .map(|m| m.message().message())
            .collect::<Vec<_>>()
    );

    // Connecting pins of different nets merges them.
    let r = connect(&mut editor, Some("OUT"), vec![c1_1, c2_1]);
    assert_eq!(r.merged_nets, ["VCC"]);
    assert_eq!(net_of(editor.project(), vcc_net).as_deref(), Some("OUT"));
    editor.undo().unwrap();
    assert_eq!(net_of(editor.project(), vcc_net).as_deref(), Some("VCC"));

    // Disconnecting one end of a direct wire keeps the other end on the
    // net (with a label), and is undoable.
    editor
        .execute(DisconnectSignals {
            signals: vec![r1_2],
        })
        .unwrap();
    let p = editor.project();
    assert_eq!(net_of(p, r1_2), None);
    assert_eq!(net_of(p, c1_1).as_deref(), Some("OUT"));
    editor.undo().unwrap();
    assert_eq!(net_of(editor.project(), r1_2).as_deref(), Some("OUT"));

    // Devices: packed inside the outline without overlap, supply symbols
    // (schematic-only) skipped.
    let board = editor
        .execute(AddBoard::new(ElementName::new("default").unwrap()))
        .unwrap()
        .board;
    editor
        .execute(SetBoardOutline::rect(
            Some(board),
            mm(0.0, 0.0),
            mm(20.0, 15.0),
        ))
        .unwrap();
    let placed = editor.execute(AutoPlaceDevices::default()).unwrap();
    assert_eq!(placed.placed.len(), 3, "{placed:?}");
    assert!(placed.outside.is_empty());
    assert_eq!(placed.skipped.len(), 2);
    let p = editor.project();
    let b = p.board(board).unwrap();
    let rects: Vec<Rect> = b
        .devices()
        .values()
        .map(|d| device_rect(p, d).unwrap())
        .collect();
    let outline = Rect {
        min: mm(0.0, 0.0),
        max: mm(20.0, 15.0),
    };
    for (i, r) in rects.iter().enumerate() {
        assert!(outline.contains(r), "{r:?}");
        for other in &rects[i + 1..] {
            assert!(!r.intersects(other), "{r:?} {other:?}");
        }
    }
    // Too small: devices which do not fit are reported.
    editor.undo().unwrap();
    editor
        .execute(SetBoardOutline::rect(
            Some(board),
            mm(0.0, 0.0),
            mm(4.0, 4.0),
        ))
        .unwrap();
    let placed = editor.execute(AutoPlaceDevices::default()).unwrap();
    assert!(!placed.outside.is_empty());
}
