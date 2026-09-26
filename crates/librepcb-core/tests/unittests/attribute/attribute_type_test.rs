//! Port of tests/unittests/core/attribute/attributetypetest.cpp.

use librepcb_core::attribute::AttributeType;
use librepcb_core::serialization::{Mode, ToSExpression};

#[test]
fn test_serialize() {
    let bytes = AttributeType::Voltage
        .to_sexpression()
        .to_byte_array(Mode::LibrePcb)
        .unwrap();
    assert_eq!(bytes, b"voltage\n");
}
