//! Tests of the undo stack (upstream `tests/unittests/editor/undostacktest.cpp`
//! works on dummy commands; here groups of real mutations are used).

use librepcb_core::project::circuit::NetClass;
use librepcb_core::project::{Mutation, NetClassId};
use librepcb_core::types::{ElementName, Uuid};
use librepcb_editor::commands::{AddNetClass, ApplyMutations};
use librepcb_editor::{Error, ProjectEditor};

use crate::helpers::create_project;

fn net_class(name: &str) -> Mutation {
    Mutation::AddNetClass(NetClass::new(
        Uuid::new_random(),
        ElementName::new(name).unwrap(),
    ))
}

fn editor() -> (tempfile::TempDir, ProjectEditor) {
    let tmp = tempfile::tempdir().unwrap();
    let editor = ProjectEditor::new(create_project(&tmp.path().join("p")));
    (tmp, editor)
}

fn class_count(editor: &ProjectEditor) -> usize {
    editor.project().circuit().net_classes().len()
}

#[test]
fn test_initial_state() {
    let (_tmp, editor) = editor();
    let stack = editor.undo_stack();
    assert!(!stack.can_undo());
    assert!(!stack.can_redo());
    assert!(stack.is_clean());
    assert!(!stack.is_group_active());
    assert_eq!(stack.undo_text(), None);
    assert!(stack.history().is_empty());
}

#[test]
fn test_execute_undo_redo() {
    let (_tmp, mut editor) = editor();
    let initial = class_count(&editor);
    let id = editor
        .execute(AddNetClass {
            name: ElementName::new("power").unwrap(),
            default_trace_width: None,
            default_via_drill: None,
        })
        .unwrap();
    assert_eq!(class_count(&editor), initial + 1);
    assert!(editor.project().circuit().net_class(id).is_some());
    assert_eq!(editor.undo_stack().undo_text(), Some("Add netclass"));
    assert!(!editor.is_clean());

    let state = editor.undo_stack().state_id();
    assert!(editor.undo().unwrap());
    assert_eq!(class_count(&editor), initial);
    assert!(editor.is_clean());
    assert_eq!(editor.undo_stack().redo_text(), Some("Add netclass"));
    assert!(!editor.undo().unwrap());

    assert!(editor.redo().unwrap());
    assert_eq!(class_count(&editor), initial + 1);
    assert!(editor.project().circuit().net_class(id).is_some());
    assert_eq!(editor.undo_stack().state_id(), state);
    assert!(!editor.redo().unwrap());
}

#[test]
fn test_group_commit_and_abort() {
    let (_tmp, mut editor) = editor();
    let initial = class_count(&editor);
    editor.begin_group("Agent action").unwrap();
    assert!(matches!(editor.begin_group("x"), Err(Error::GroupActive)));
    editor.apply_mutations("a", vec![net_class("a")]).unwrap();
    editor.apply_mutations("b", vec![net_class("b")]).unwrap();
    assert!(!editor.undo_stack().can_undo());
    assert!(editor.commit_group().unwrap());
    assert_eq!(class_count(&editor), initial + 2);
    assert_eq!(editor.undo_stack().history().len(), 1);
    assert_eq!(editor.undo_stack().undo_text(), Some("Agent action"));

    // One undo reverts the whole group.
    editor.undo().unwrap();
    assert_eq!(class_count(&editor), initial);

    // Abort rolls back.
    editor.begin_group("aborted").unwrap();
    editor.apply_mutations("c", vec![net_class("c")]).unwrap();
    assert_eq!(class_count(&editor), initial + 1);
    editor.abort_group().unwrap();
    assert_eq!(class_count(&editor), initial);
    assert!(matches!(editor.abort_group(), Err(Error::NoGroupActive)));

    // Empty groups are dropped.
    editor.begin_group("empty").unwrap();
    assert!(!editor.commit_group().unwrap());
    // The redo entry is still there (upstream would have discarded it
    // when the aborted group was started).
    assert!(editor.undo_stack().can_redo());
}

#[test]
fn test_failed_command_rolls_back() {
    let (_tmp, mut editor) = editor();
    let before = editor.project().circuit().clone();
    let duplicate = NetClass::new(Uuid::new_random(), ElementName::new("dup").unwrap());
    let result = editor.execute(ApplyMutations {
        text: None,
        mutations: vec![
            net_class("ok"),
            Mutation::AddNetClass(duplicate.clone()),
            Mutation::AddNetClass(duplicate),
        ],
    });
    assert!(result.is_err());
    assert_eq!(*editor.project().circuit(), before);
    assert!(!editor.undo_stack().can_undo());

    // Inside a group, only the failed command is rolled back.
    editor.begin_group("group").unwrap();
    editor.apply_mutations("x", vec![net_class("x")]).unwrap();
    assert!(
        editor
            .apply_mutations("y", vec![net_class("y"), net_class("y")])
            .is_err()
    );
    assert!(editor.commit_group().unwrap());
    let names: Vec<_> = editor
        .project()
        .circuit()
        .net_classes()
        .values()
        .map(|c| c.name().to_string())
        .collect();
    assert!(names.contains(&"x".to_owned()));
    assert!(!names.contains(&"y".to_owned()));
}

#[test]
fn test_new_group_discards_redo_and_clean_state() {
    let (_tmp, mut editor) = editor();
    editor.apply_mutations("a", vec![net_class("a")]).unwrap();
    editor.save().unwrap();
    assert!(editor.is_clean());
    editor.undo().unwrap();
    assert!(!editor.is_clean());
    editor.apply_mutations("b", vec![net_class("b")]).unwrap();
    assert!(!editor.undo_stack().can_redo());
    // The saved state is no longer reachable.
    editor.undo().unwrap();
    assert!(!editor.is_clean());
    let _ = NetClassId(Uuid::new_random());
}
