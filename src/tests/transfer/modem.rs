use super::*;

#[test]
fn crc16_check_value() {
    assert_eq!(crc16(b"123456789"), 0x31C3);
}
