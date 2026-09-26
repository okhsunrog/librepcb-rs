//! Port of tests/unittests/core/library/librarytest.cpp.

use librepcb_core::library::cat::{ComponentCategory, PackageCategory};
use librepcb_core::library::cmp::Component;
use librepcb_core::library::dev::Device;
use librepcb_core::library::org::Organization;
use librepcb_core::library::sym::Symbol;
use librepcb_core::library::{Library, LibraryBaseElement};

use super::{assert_migration_required, assert_open_save_reopen, data_path, open_dir};

// The whitespaces in the path are there to make the test even stronger ;)
const DEST_NAME: &str = "test dir.lplib";

#[test]
fn test_upgrade_v01() {
    assert_migration_required::<Library>("libraries/v0.1.lplib", DEST_NAME);
    assert_open_save_reopen::<Library>("libraries/Populated Library.lplib", DEST_NAME);
}

#[test]
fn test_open_empty_library() {
    let lib = Library::open(open_dir(&data_path("libraries/Empty Library.lplib"), false)).unwrap();
    assert!(lib.search_for_elements::<Symbol>().is_empty());
}

#[test]
fn test_invalid_suffix() {
    let (_tmp, dir) = super::copy_to_temp("libraries/Empty Library.lplib", "lib");
    let err = Library::open(open_dir(&dir, false)).unwrap_err();
    assert!(err.to_string().contains("suffix '.lplib'"), "{err}");
}

#[test]
fn test_search_for_elements() {
    let lib = Library::open(open_dir(
        &data_path("libraries/Populated Library.lplib"),
        false,
    ))
    .unwrap();
    assert_eq!(lib.manufacturer().as_str(), "Demo Manufacturer");
    assert_eq!(
        lib.url(),
        "https://github.com/LibrePCB/PopulatedLibrary.lplib"
    );
    assert!(!lib.icon().is_empty());
    let counts = [
        lib.search_for_elements::<ComponentCategory>().len(),
        lib.search_for_elements::<PackageCategory>().len(),
        lib.search_for_elements::<Symbol>().len(),
        lib.search_for_elements::<Component>().len(),
        lib.search_for_elements::<Device>().len(),
        lib.search_for_elements::<Organization>().len(),
    ];
    assert_eq!(counts, [11, 4, 19, 15, 8, 2]);
    for path in lib.search_for_elements::<Symbol>() {
        assert!(path.starts_with("sym/"), "{path}");
    }
}
