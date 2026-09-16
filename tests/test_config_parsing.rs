use std::io::Cursor;
use pam_linuxcampam::config::{AuthPolicy, Configuration, ProximitySensorMode};

fn load_config(ini: &str) -> Configuration {
    let mut config = Configuration::new();
    assert!(config.load(Cursor::new(ini), None));
    config
}

#[test]
fn fallback_from_auth_to_general() {
    let config = load_config(
        r#"
[General]
threshold=0.85
detection_threshold=0.95
timeout_ms=5000
auth_method=strict
max_embeddings=10
"#,
    );

    assert_eq!(config.threshold, 0.85);
    assert_eq!(config.detection_threshold, 0.95);
    assert_eq!(config.timeout_ms, 5000);
    assert_eq!(config.policy, AuthPolicy::StrictAll);
    assert_eq!(config.max_embeddings, 10);
}

#[test]
fn auth_overrides_general() {
    let config = load_config(
        r#"
[General]
threshold=0.5
detection_threshold=0.5
timeout_ms=1000
auth_method=lenient
max_embeddings=2

[Auth]
threshold=0.99
detection_threshold=0.99
timeout_ms=8000
policy=strict
max_embeddings=20
"#,
    );

    assert_eq!(config.threshold, 0.99);
    assert_eq!(config.detection_threshold, 0.99);
    assert_eq!(config.timeout_ms, 8000);
    assert_eq!(config.policy, AuthPolicy::StrictAll);
    assert_eq!(config.max_embeddings, 20);
}

#[test]
fn handles_malformed_numbers() {
    let config = load_config(
        r#"
[Auth]
threshold=bad_float
timeout_ms=not_an_int
max_embeddings=-invalid
"#,
    );

    assert_eq!(config.threshold, Configuration::DEFAULT_THRESHOLD);
    assert_eq!(config.timeout_ms, Configuration::DEFAULT_TIMEOUT_MS);
    assert_eq!(config.max_embeddings, Configuration::DEFAULT_MAX_EMBEDDINGS);
}

#[test]
fn proximity_sensor_settings() {
    let config = load_config(
        r#"
[Hardware]
proximity_sensor=enabled
proximity_sensor_id=TEST_ID
proximity_enforce=true

[Proximity]
wake_enabled=false
always_wake_on_presence_detected=false
wake_confidence_threshold=80
lock_enabled=true
lock_confidence_threshold=10
lock_timeout_seconds=5
lock_command=custom_lock
"#,
    );

    assert_eq!(config.proximity_sensor, ProximitySensorMode::Enabled);
    assert_eq!(config.proximity_sensor_id, "TEST_ID");
    assert!(config.proximity_enforce);

    assert!(!config.wake_enabled);
    assert!(!config.always_wake_on_presence_detected);
    assert_eq!(config.wake_confidence_threshold, 80);

    assert!(config.lock_enabled);
    assert_eq!(config.lock_confidence_threshold, 10);
    assert_eq!(config.lock_timeout_seconds, 5);
    assert_eq!(config.lock_command, "custom_lock");
}

#[test]
fn provider_priority_parsing() {
    let config = load_config(
        r#"
[Hardware]
provider_priority=TensorRT, CUDA, CPU
"#,
    );

    assert_eq!(config.provider_priority.len(), 3);
    assert_eq!(config.provider_priority[0], "TensorRT");
    assert_eq!(config.provider_priority[1], "CUDA");
    assert_eq!(config.provider_priority[2], "CPU");
}

#[test]
fn empty_provider_priority_uses_default() {
    let config = load_config(
        r#"
[Hardware]
provider_priority=
"#,
    );

    assert_eq!(config.provider_priority.len(), 2);
    assert_eq!(config.provider_priority[0], "OpenCL");
    assert_eq!(config.provider_priority[1], "CPU");
}

#[test]
fn explicit_camera_definitions() {
    let config = load_config(
        r#"
[Cameras]
names=cam1, cam2

[Camera.cam1]
path=/dev/video1
type=rgb
mandatory=true
min_brightness=10

[Camera.cam2]
path=/dev/video2
type=ir
enroll_hdr=true
enroll_averaging=true
enroll_average_frames=5
"#,
    );

    assert_eq!(config.camera_defs.len(), 2);

    assert_eq!(config.camera_defs[0].id, "cam1");
    assert_eq!(config.camera_defs[0].path, "/dev/video1");
    assert_eq!(config.camera_defs[0].cam_type, "rgb");
    assert!(config.camera_defs[0].mandatory);
    assert_eq!(config.camera_defs[0].min_brightness, 10);

    assert_eq!(config.camera_defs[1].id, "cam2");
    assert_eq!(config.camera_defs[1].path, "/dev/video2");
    assert_eq!(config.camera_defs[1].cam_type, "ir");
    assert!(!config.camera_defs[1].mandatory);
    assert_eq!(config.camera_defs[1].enroll_hdr, "true");
    assert_eq!(config.camera_defs[1].enroll_averaging, "true");
    assert_eq!(config.camera_defs[1].enroll_average_frames, 5);
}

#[test]
fn to_string_output_contains_key_elements() {
    let config = load_config(
        r#"
[General]
log_level=debug
[Auth]
policy=lenient
[Hardware]
proximity_sensor=disabled
"#,
    );

    let out = config.to_string_formatted();
    assert!(out.contains("Log Level: debug"));
    assert!(out.contains("Lenient (Any Camera Match)"));
    assert!(out.contains("Proximity Sensor: disabled"));
}

#[test]
fn min_uid_parsing() {
    let config = load_config(
        r#"
[Security]
min_uid=2000
"#,
    );
    assert_eq!(config.min_uid, 2000);
}

#[test]
fn negative_min_uid_fallback() {
    let config = load_config(
        r#"
[Security]
min_uid=-5
"#,
    );
    assert_eq!(config.min_uid, pam_linuxcampam::DEFAULT_MIN_UID);
}

#[test]
fn performance_settings() {
    let config = load_config(
        r#"
[Performance]
gpu_flush=off
gpu_throttle_ms=50
model_keep_alive_sec=120
"#,
    );
    assert!(!config.gpu_flush);
    assert_eq!(config.gpu_throttle_ms, 50);
    assert_eq!(config.model_keep_alive_sec, 120);
}
