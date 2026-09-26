//! Port of tests/unittests/core/attribute/attributeunittest.cpp.

use librepcb_core::attribute::AttributeUnit;
use librepcb_core::serialization::{Mode, ToSExpression};

#[test]
fn test_serialize() {
    let unit = AttributeUnit::new("volt", "V", &[]);
    let bytes = unit.to_sexpression().to_byte_array(Mode::LibrePcb).unwrap();
    assert_eq!(bytes, b"volt\n");
}
