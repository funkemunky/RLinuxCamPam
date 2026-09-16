use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use pam_linuxcampam::auth_engine::AuthEngine;
use pam_linuxcampam::camera::{Camera, ICamera};
use pam_linuxcampam::config::Configuration;
use pam_linuxcampam::logger::Logger;

#[test]
fn test_config_resilience() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let config_path = temp.path();
    {
        let mut out = File::create(config_path).expect("failed to create config");
        writeln!(out, "[General]").unwrap();
        writeln!(out, "detection_threshold = invalid_number").unwrap();
        writeln!(out, "log_level = DEBUG").unwrap();
        writeln!(out, "[Camera]").unwrap();
    }

    let mut engine = AuthEngine::new();
    let _ = engine.init(config_path);
    assert_eq!(
        engine.get_config().detection_threshold,
        Configuration::DEFAULT_DETECTION_THRESHOLD
    );
    assert_eq!(engine.get_config().log_level, "DEBUG");
}

#[test]
fn test_camera_open_failure() {
    if std::env::var("TEST_IN_DOCKER").is_ok() {
        return;
    }

    let mut cam = Camera::new("/dev/video999", false, None);
    let frame = cam.capture();
    assert!(frame.is_none());
}

use std::sync::Mutex;

static LOGGER_TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_ir_emitter_invalid_path() {
    if std::env::var("TEST_IN_DOCKER").is_ok() {
        return;
    }

    let _lock = LOGGER_TEST_MUTEX.lock().unwrap();

    let temp_log = tempfile::NamedTempFile::new().unwrap();
    let log_path = temp_log.path().to_str().unwrap();
    Logger::set_log_file(log_path);

    let mut cam = Camera::new("/dev/null", true, Some(Path::new("/tmp/non_existent_executable")));
    cam.trigger_ir_emitter();

    let output = fs::read_to_string(log_path).unwrap_or_default();
    let spawn_failed = output.contains("posix_spawn failed");
    let exec_failed = output.contains("IR emitter exited with code: 127");
    assert!(spawn_failed || exec_failed, "Output was: {output}");
}

#[test]
fn test_ir_emitter_non_executable() {
    if std::env::var("TEST_IN_DOCKER").is_ok() {
        return;
    }

    let _lock = LOGGER_TEST_MUTEX.lock().unwrap();

    let dummy = tempfile::NamedTempFile::new().unwrap();
    let dummy_path = dummy.path();
    {
        let mut out = File::create(dummy_path).expect("failed to create dummy script");
        writeln!(out, "#!/bin/bash\nexit 0").unwrap();
    }

    let temp_log = tempfile::NamedTempFile::new().unwrap();
    let log_path = temp_log.path().to_str().unwrap();
    Logger::set_log_file(log_path);

    let mut cam = Camera::new("/dev/null", true, Some(dummy_path));
    cam.trigger_ir_emitter();

    let output = fs::read_to_string(log_path).unwrap_or_default();
    let spawn_failed = output.contains("posix_spawn failed");
    let exec_failed = output.contains("IR emitter exited with code:");
    assert!(spawn_failed || exec_failed, "Output was: {output}");
}
