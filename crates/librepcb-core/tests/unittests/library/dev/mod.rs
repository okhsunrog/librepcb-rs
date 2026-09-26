//! Port of tests/unittests/core/library/dev/devicetest.cpp.

use librepcb_core::library::dev::Device;

use super::{assert_migration_required, assert_open_save_reopen};

const UUID: &str = "4f5ee784-4b1b-407c-802b-44625163d90f";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<Device>(&format!("libraries/v0.1.lplib/dev/{UUID}"), UUID);
    assert_open_save_reopen::<Device>(
        &format!("libraries/Populated Library.lplib/dev/{UUID}"),
        UUID,
    );
}
