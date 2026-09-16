use crate::sensor_parser::{SensorParser, SensorState};

pub struct Ite8353Parser;

impl Ite8353Parser {
    pub const PACKET_SIZE: usize = 12;
    pub const MAGIC_BYTE: u8 = 0x02;

    pub fn new() -> Self {
        Self
    }
}

impl Default for Ite8353Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorParser for Ite8353Parser {
    fn parse_payload(&self, buffer: &[u8]) -> Option<SensorState> {
        if buffer.len() < Self::PACKET_SIZE {
            return None;
        }

        let magic = buffer[0];
        if magic == Self::MAGIC_BYTE {
            let confidence = buffer[7];
            let presence = buffer[11];
            Some(SensorState {
                confidence_cm: confidence as i32,
                human_present: presence == 1,
            })
        } else {
            None
        }
    }
}
