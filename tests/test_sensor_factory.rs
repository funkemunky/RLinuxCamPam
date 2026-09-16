use pam_linuxcampam::sensor_factory::{ISensorFactory, SensorFactory};

#[test]
fn create_known_parser() {
    let factory = SensorFactory::new();
    let parser = factory.create("ITE8353");
    assert!(parser.is_some());
}

#[test]
fn create_unknown_parser_returns_null() {
    let factory = SensorFactory::new();
    let parser = factory.create("UNKNOWN_SENSOR");
    assert!(parser.is_none());
}
