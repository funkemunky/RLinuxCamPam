#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorState {
    pub confidence_cm: i32,
    pub human_present: bool,
}

pub trait SensorParser: Send + Sync {
    fn parse_payload(&self, buffer: &[u8]) -> Option<SensorState>;
}
