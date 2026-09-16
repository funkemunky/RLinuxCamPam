use pam_linuxcampam::parsers::Ite8353Parser;
use pam_linuxcampam::sensor_parser::SensorParser;

const EXPECTED_PAYLOAD_SIZE: usize = 12;
const SHORT_PAYLOAD_SIZE: usize = 11;
const TEST_CONFIDENCE: u8 = 85;

#[test]
fn parse_valid_payload() {
    let parser = Ite8353Parser::new();
    let mut buffer = [0u8; EXPECTED_PAYLOAD_SIZE];
    buffer[0] = 0x02; // Magic 0x02
    buffer[7] = TEST_CONFIDENCE;
    buffer[11] = 1; // Presence 1

    let result = parser.parse_payload(&buffer);
    assert!(result.is_some());
    let state = result.unwrap();
    assert_eq!(state.confidence_cm, TEST_CONFIDENCE as i32);
    assert!(state.human_present);
}

#[test]
fn parse_invalid_magic_byte() {
    let parser = Ite8353Parser::new();
    let mut buffer = [0u8; EXPECTED_PAYLOAD_SIZE];
    buffer[0] = 0x03; // Magic 0x03 (invalid)
    buffer[7] = TEST_CONFIDENCE;
    buffer[11] = 1;

    let result = parser.parse_payload(&buffer);
    assert!(result.is_none());
}

#[test]
fn parse_too_small_buffer() {
    let parser = Ite8353Parser::new();
    let mut buffer = [0u8; SHORT_PAYLOAD_SIZE];
    buffer[0] = 0x02;
    buffer[7] = TEST_CONFIDENCE;

    let result = parser.parse_payload(&buffer);
    assert!(result.is_none());
}

#[test]
fn empty_buffer() {
    let parser = Ite8353Parser::new();
    let result = parser.parse_payload(&[]);
    assert!(result.is_none());
}
