use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use pam_linuxcampam::hardware_manager::HardwareId;
use pam_linuxcampam::presence_tripwire::{
    HidDevicePtr, PresenceTripwire, PresenceTripwireHidOps,
};
use pam_linuxcampam::sensor_factory::ISensorFactory;
use pam_linuxcampam::sensor_parser::{SensorParser, SensorState};

struct MockSensorParser {
    state: SensorState,
}

impl SensorParser for MockSensorParser {
    fn parse_payload(&self, _buffer: &[u8]) -> Option<SensorState> {
        Some(self.state)
    }
}

struct ParsingFactory {
    state: SensorState,
}

impl ISensorFactory for ParsingFactory {
    fn create(&self, _hardware_id: &str) -> Option<Box<dyn SensorParser>> {
        Some(Box::new(MockSensorParser { state: self.state }))
    }
}

struct NullSensorFactory;

impl ISensorFactory for NullSensorFactory {
    fn create(&self, _hardware_id: &str) -> Option<Box<dyn SensorParser>> {
        None
    }
}

fn make_passthrough_ops() -> PresenceTripwireHidOps {
    PresenceTripwireHidOps {
        init: Arc::new(|| 0),
        open_path: Arc::new(|_| 0x1234 as HidDevicePtr),
        read_timeout: Arc::new(|_, _, _| 0),
        close_fn: Arc::new(|_| {}),
        exit_fn: Arc::new(|| {}),
    }
}

#[test]
fn start_fails_with_invalid_sensor_id() {
    let factory = Arc::new(NullSensorFactory);
    let mut tripwire = PresenceTripwire::new(factory, PresenceTripwireHidOps::default());
    let hw_id = HardwareId::new("ValidId").unwrap();
    let result = tripwire.start("/dev/invalid_hidraw_node", &hw_id, |_, _| {});
    assert!(!result);
}

#[test]
fn stop_without_start_is_safe() {
    let factory = Arc::new(NullSensorFactory);
    let mut tripwire = PresenceTripwire::new(factory, PresenceTripwireHidOps::default());
    tripwire.stop();
}

#[test]
fn safe_destruction() {
    let factory = Arc::new(NullSensorFactory);
    let _tripwire = PresenceTripwire::new(factory, PresenceTripwireHidOps::default());
}

#[test]
fn start_returns_true_with_working_hid() {
    let factory = Arc::new(ParsingFactory {
        state: SensorState {
            confidence_cm: 100,
            human_present: true,
        },
    });
    let mut tripwire = PresenceTripwire::new(factory, make_passthrough_ops());
    let hw_id = HardwareId::new("ValidId").unwrap();
    assert!(tripwire.start("/dev/fake", &hw_id, |_, _| {}));
    tripwire.stop();
}

#[test]
fn hid_init_failure_prevents_start() {
    let factory = Arc::new(ParsingFactory {
        state: SensorState {
            confidence_cm: 100,
            human_present: true,
        },
    });
    let mut ops = make_passthrough_ops();
    ops.init = Arc::new(|| -1);

    let mut tripwire = PresenceTripwire::new(factory, ops);
    let hw_id = HardwareId::new("ValidId").unwrap();
    assert!(!tripwire.start("/dev/fake", &hw_id, |_, _| {}));
}

#[test]
fn hid_open_failure_prevents_start() {
    let factory = Arc::new(ParsingFactory {
        state: SensorState {
            confidence_cm: 100,
            human_present: true,
        },
    });
    let mut ops = make_passthrough_ops();
    ops.open_path = Arc::new(|_| std::ptr::null_mut());

    let mut tripwire = PresenceTripwire::new(factory, ops);
    let hw_id = HardwareId::new("ValidId").unwrap();
    assert!(!tripwire.start("/dev/fake", &hw_id, |_, _| {}));
}

#[test]
fn poll_loop_fires_callback_on_presence() {
    const EXPECTED_CONFIDENCE: i32 = 100;
    let factory = Arc::new(ParsingFactory {
        state: SensorState {
            confidence_cm: EXPECTED_CONFIDENCE,
            human_present: true,
        },
    });

    let data_sent = Arc::new(AtomicBool::new(false));
    let ds_clone = Arc::clone(&data_sent);
    let mut ops = make_passthrough_ops();
    ops.read_timeout = Arc::new(move |_, buf, _| {
        if ds_clone.load(Ordering::Relaxed) {
            0
        } else {
            ds_clone.store(true, Ordering::Relaxed);
            buf[0] = 0x01;
            1
        }
    });

    let (tx, rx) = channel();
    let mut tripwire = PresenceTripwire::new(factory, ops);
    let hw_id = HardwareId::new("ValidId").unwrap();

    assert!(tripwire.start("/dev/fake", &hw_id, move |present, conf| {
        let _ = tx.send((present, conf));
    }));

    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(received.0);
    assert_eq!(received.1, EXPECTED_CONFIDENCE);
    tripwire.stop();
}

#[test]
fn poll_loop_aborts_on_hid_error() {
    let factory = Arc::new(ParsingFactory {
        state: SensorState {
            confidence_cm: 100,
            human_present: true,
        },
    });
    let callback_called = Arc::new(AtomicBool::new(false));
    let cb_clone = Arc::clone(&callback_called);

    let mut ops = make_passthrough_ops();
    ops.read_timeout = Arc::new(|_, _, _| -1);

    {
        let mut tripwire = PresenceTripwire::new(factory, ops);
        let hw_id = HardwareId::new("ValidId").unwrap();
        assert!(tripwire.start("/dev/fake", &hw_id, move |_, _| {
            cb_clone.store(true, Ordering::SeqCst);
        }));
    } // Thread joined on drop

    assert!(!callback_called.load(Ordering::SeqCst));
}
