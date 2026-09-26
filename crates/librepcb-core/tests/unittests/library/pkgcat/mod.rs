//! Port of tests/unittests/core/library/pkgcat/packagecategorytest.cpp.

use librepcb_core::library::cat::PackageCategory;

use super::{assert_migration_required, assert_open_save_reopen};

const UUID: &str = "a20f0330-06d3-4bc2-a1fa-f8577deb6770";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<PackageCategory>(
        &format!("libraries/v0.1.lplib/pkgcat/{UUID}"),
        UUID,
    );
    assert_open_save_reopen::<PackageCategory>(
        &format!("libraries/Populated Library.lplib/pkgcat/{UUID}"),
        UUID,
    );
}
