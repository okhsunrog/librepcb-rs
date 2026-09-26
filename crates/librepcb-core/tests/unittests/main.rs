//! Port of the upstream unit tests in tests/unittests/core/types,
//! tests/unittests/core/serialization, tests/unittests/core/fileio and
//! tests/unittests/core/systeminfotest.cpp (same test vectors; parametrized
//! gtest suites are loops over the data tables).

mod fileio;
mod helpers;
mod serialization;
mod system_info_test;
mod types;

/// Not a test: the "dummy-binary" process used by the systeminfo and
/// directory lock tests (see [`helpers::spawn_dummy_process()`]). Sleeps only
/// if started by the tests.
#[test]
#[ignore = "helper process for other tests"]
fn dummy_binary() {
    if std::env::var_os(helpers::DUMMY_BINARY_ENV).is_some() {
        std::thread::sleep(std::time::Duration::from_secs(120));
    }
}
