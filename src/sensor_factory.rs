use std::collections::HashMap;
use std::sync::Arc;
use crate::parsers::Ite8353Parser;
use crate::sensor_parser::SensorParser;

pub trait ISensorFactory: Send + Sync {
    fn create(&self, hardware_id: &str) -> Option<Box<dyn SensorParser>>;
}

pub struct SensorFactory {
    registry: HashMap<String, Arc<dyn Fn() -> Box<dyn SensorParser> + Send + Sync>>,
}

impl SensorFactory {
    pub fn new() -> Self {
        let mut registry: HashMap<String, Arc<dyn Fn() -> Box<dyn SensorParser> + Send + Sync>> =
            HashMap::new();
        registry.insert(
            "ITE8353".to_string(),
            Arc::new(|| Box::new(Ite8353Parser::new())),
        );
        Self { registry }
    }
}

impl Default for SensorFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ISensorFactory for SensorFactory {
    fn create(&self, hardware_id: &str) -> Option<Box<dyn SensorParser>> {
        self.registry.get(hardware_id).map(|creator| creator())
    }
}
