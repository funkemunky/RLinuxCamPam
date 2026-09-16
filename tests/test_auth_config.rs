use std::fs;
use pam_linuxcampam::config::Configuration;
use pam_linuxcampam::utils::is_valid_username;
use tempfile::TempDir;

#[test]
fn uses_defaults_when_config_missing() {
    let mut config = Configuration::new();
    let result = config.load_file("non_existent_file.ini", None);
    assert!(result);

    assert_eq!(config.threshold, Configuration::DEFAULT_THRESHOLD);
    assert_eq!(config.timeout_ms, Configuration::DEFAULT_TIMEOUT_MS);
    assert_eq!(config.lockout_attempts, Configuration::DEFAULT_LOCKOUT_ATTEMPTS);
    assert_eq!(config.gpu_throttle_ms, Configuration::DEFAULT_GPU_THROTTLE_MS);

    assert!(!config.provider_priority.is_empty());
    assert_eq!(config.provider_priority[0], "OpenCL");
}

#[test]
fn parses_valid_values() {
    let temp = TempDir::new().unwrap();
    let cfg_path = temp.path().join("config_test.ini");

    let content = r#"
[General]
threshold = 0.75
detection_threshold = 0.85
timeout_ms = 5000
max_embeddings = 10

[Hardware]
provider_priority = CUDA,OpenVINO,OpenCL,CPU

[Security]
lockout_attempts = 3
lockout_duration_sec = 600

[Performance]
gpu_flush = off
gpu_throttle_ms = 50
model_keep_alive_sec = 120
"#;
    fs::write(&cfg_path, content).unwrap();

    let mut config = Configuration::new();
    assert!(config.load_file(&cfg_path, None));

    assert_eq!(config.threshold, 0.75);
    assert_eq!(config.detection_threshold, 0.85);
    assert_eq!(config.timeout_ms, 5000);
    assert_eq!(config.max_embeddings, 10);

    assert_eq!(config.provider_priority.len(), 4);
    assert_eq!(config.provider_priority[0], "CUDA");
    assert_eq!(config.provider_priority[1], "OpenVINO");

    assert_eq!(config.lockout_attempts, 3);
    assert_eq!(config.lockout_duration_sec, 600);

    assert!(!config.gpu_flush);
    assert_eq!(config.gpu_throttle_ms, 50);
    assert_eq!(config.model_keep_alive_sec, 120);
}

#[test]
fn parses_proximity_config() {
    let temp = TempDir::new().unwrap();
    let cfg_path = temp.path().join("config_test.ini");

    let content = r#"
[Hardware]
proximity_sensor_id = MY_SENSOR

[Proximity]
wake_enabled = true
wake_confidence_threshold = 70
lock_enabled = true
lock_confidence_threshold = 10
lock_timeout_seconds = 30
lock_command = custom_lock_cmd
"#;
    fs::write(&cfg_path, content).unwrap();

    let mut config = Configuration::new();
    assert!(config.load_file(&cfg_path, None));

    assert_eq!(config.proximity_sensor_id, "MY_SENSOR");
    assert!(config.wake_enabled);
    assert_eq!(config.wake_confidence_threshold, 70);
    assert!(config.lock_enabled);
    assert_eq!(config.lock_confidence_threshold, 10);
    assert_eq!(config.lock_timeout_seconds, 30);
    assert_eq!(config.lock_command, "custom_lock_cmd");
}

#[test]
fn handles_partial_config() {
    let temp = TempDir::new().unwrap();
    let cfg_path = temp.path().join("config_test.ini");

    let content = r#"
[General]
threshold = 0.65
"#;
    fs::write(&cfg_path, content).unwrap();

    let mut config = Configuration::new();
    assert!(config.load_file(&cfg_path, None));

    assert_eq!(config.threshold, 0.65);
    assert_eq!(config.lockout_attempts, Configuration::DEFAULT_LOCKOUT_ATTEMPTS);
}

#[test]
fn parses_logging_config() {
    let temp = TempDir::new().unwrap();
    let cfg_path = temp.path().join("config_test.ini");

    let content = r#"
[General]
log_level = debug
log_file = /tmp/linuxcampam.log
"#;
    fs::write(&cfg_path, content).unwrap();

    let mut config = Configuration::new();
    assert!(config.load_file(&cfg_path, None));

    assert_eq!(config.log_level, "debug");
    assert_eq!(config.log_file, "/tmp/linuxcampam.log");
}

#[test]
fn fallback_on_invalid_data() {
    let temp = TempDir::new().unwrap();
    let cfg_path = temp.path().join("config_test.ini");

    let content = r#"
[General]
threshold = invalid_float
timeout_ms = invalid_int
[Security]
lockout_attempts = bad_number
"#;
    fs::write(&cfg_path, content).unwrap();

    let mut config = Configuration::new();
    assert!(config.load_file(&cfg_path, None));

    assert_eq!(config.threshold, Configuration::DEFAULT_THRESHOLD);
    assert_eq!(config.lockout_attempts, Configuration::DEFAULT_LOCKOUT_ATTEMPTS);
}

#[test]
fn username_sanitization_algorithm() {
    assert!(is_valid_username("vlad"));
    assert!(is_valid_username("user.name"));
    assert!(!is_valid_username("../../etc/passwd"));
    assert!(!is_valid_username("user; rm -rf /"));
}
