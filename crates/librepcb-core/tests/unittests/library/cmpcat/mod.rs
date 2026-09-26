//! Port of tests/unittests/core/library/cmpcat/componentcategorytest.cpp.

use librepcb_core::library::cat::ComponentCategory;

use super::{assert_migration_required, assert_open_save_reopen};

const UUID: &str = "1039f038-20a6-4bfe-89c1-99f34fbb45bd";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<ComponentCategory>(
        &format!("libraries/v0.1.lplib/cmpcat/{UUID}"),
        UUID,
    );
    assert_open_save_reopen::<ComponentCategory>(
        &format!("libraries/Populated Library.lplib/cmpcat/{UUID}"),
        UUID,
    );
}
