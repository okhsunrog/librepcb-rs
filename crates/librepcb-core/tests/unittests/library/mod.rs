//! Ports of tests/unittests/core/library/*.cpp (except packages), plus tests
//! of the library element checks.
//!
//! The upstream `testUpgradeV01` tests open a v0.1 element, which requires
//! the (not yet ported) file format migrations. They are ported as a check
//! that such elements are rejected ([`assert_migration_required()`]) plus the
//! same open/save/reopen cycle on the current-format copy of the element in
//! `Populated Library.lplib` ([`assert_open_save_reopen()`]).

mod check_test;
mod cmp;
mod cmpcat;
mod dev;
mod library_base_element_test;
mod library_test;
mod pkgcat;
mod sym;

use std::path::PathBuf;
use std::sync::Arc;

use librepcb_core::application::file_format_version;
use librepcb_core::fileio::{
    FilePath, TransactionalDirectory, TransactionalFileSystem, file_utils,
};
use librepcb_core::library::{Error, LibraryBaseElement};

use crate::helpers::TempDir;

/// Returns the upstream test data directory (see `LIBREPCB_UPSTREAM_DIR` in
/// `.cargo/config.toml`).
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("LIBREPCB_UPSTREAM_DIR")).join("tests/data")
}

/// Returns `path` (relative to the test data directory) as file path.
pub fn data_path(path: &str) -> FilePath {
    FilePath::new(data_dir().join(path).canonicalize().unwrap()).unwrap()
}

/// Opens `path` as transactional directory.
pub fn open_dir(path: &FilePath, writable: bool) -> TransactionalDirectory {
    let fs = if writable {
        TransactionalFileSystem::open_rw(path)
    } else {
        TransactionalFileSystem::open_ro(path)
    };
    TransactionalDirectory::new(Arc::new(fs.unwrap()), "")
}

/// Copies the test data directory `src` to `dest_name` in a new temporary
/// directory.
pub fn copy_to_temp(src: &str, dest_name: &str) -> (TempDir, FilePath) {
    let tmp = TempDir::new();
    let dest = tmp.path().path_to(dest_name);
    file_utils::copy_dir_recursively(&data_path(src), &dest).unwrap();
    (tmp, dest)
}

/// Upstream `testUpgradeV01`, first part: opening the v0.1 element needs a
/// file format migration, which is not supported yet.
pub fn assert_migration_required<E: LibraryBaseElement>(src: &str, dest_name: &str) {
    let (_tmp, dir) = copy_to_temp(src, dest_name);
    let version_file = dir.path_to(&format!(".librepcb-{}", E::SHORT_ELEMENT_NAME));
    assert!(
        file_utils::read_file(&version_file)
            .unwrap()
            .starts_with(b"0.1\n")
    );
    match E::open(open_dir(&dir, true)) {
        Err(Error::MigrationRequired { version, .. }) => assert_eq!(version.to_string(), "0.1"),
        other => panic!("unexpected result: {:?}", other.err()),
    }
}

/// Upstream `testUpgradeV01`, second part: open, save and re-open the
/// element (from the current-format copy `src`).
pub fn assert_open_save_reopen<E: LibraryBaseElement>(src: &str, dest_name: &str) {
    let (_tmp, dir) = copy_to_temp(src, dest_name);
    let file = dir.path_to(&format!("{}.lp", E::LONG_ELEMENT_NAME));
    let original = file_utils::read_file(&file).unwrap();
    {
        let mut obj = E::open(open_dir(&dir, true)).unwrap();
        obj.save().unwrap();
        obj.directory().file_system().save().unwrap();
    }
    let version_file = dir.path_to(&format!(".librepcb-{}", E::SHORT_ELEMENT_NAME));
    assert!(
        file_utils::read_file(&version_file)
            .unwrap()
            .starts_with(format!("{}\n", file_format_version()).as_bytes())
    );
    assert_eq!(file_utils::read_file(&file).unwrap(), original);
    E::open(open_dir(&dir, true)).unwrap();
}
