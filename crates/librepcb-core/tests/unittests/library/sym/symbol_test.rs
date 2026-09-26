//! Port of tests/unittests/core/library/sym/symboltest.cpp.

use librepcb_core::library::sym::Symbol;

use crate::library::{assert_migration_required, assert_open_save_reopen};

const UUID: &str = "35aad2af-5cd4-42ae-8576-fe7febad0d8a";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<Symbol>(&format!("libraries/v0.1.lplib/sym/{UUID}"), UUID);
    assert_open_save_reopen::<Symbol>(
        &format!("libraries/Populated Library.lplib/sym/{UUID}"),
        UUID,
    );
}
