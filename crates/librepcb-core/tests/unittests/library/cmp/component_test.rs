//! Port of tests/unittests/core/library/cmp/componenttest.cpp.

use librepcb_core::library::cmp::Component;

use crate::library::{assert_migration_required, assert_open_save_reopen};

const UUID: &str = "45022bef-9310-4aa2-92ef-acec1b95aa4e";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<Component>(&format!("libraries/v0.1.lplib/cmp/{UUID}"), UUID);
    assert_open_save_reopen::<Component>(
        &format!("libraries/Populated Library.lplib/cmp/{UUID}"),
        UUID,
    );
}
