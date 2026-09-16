use image::{Rgb, RgbImage};
use pam_linuxcampam::auth_engine::AuthEngine;
use pam_linuxcampam::camera::ICamera;
use pam_linuxcampam::config::CameraDefinition;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MOCK_FRAME_WIDTH: u32 = 640;
const MOCK_FRAME_HEIGHT: u32 = 480;
const MOCK_EMBEDDING_SIZE: usize = 128;

struct MockCamera {
    frame: Option<RgbImage>,
}

impl ICamera for MockCamera {
    fn trigger_ir_emitter(&mut self) {}
    fn capture(&mut self) -> Option<RgbImage> {
        self.frame.clone()
    }
    fn capture_averaged(&mut self, _num_frames: usize) -> Option<RgbImage> {
        self.frame.clone()
    }
    fn capture_hdr(&mut self) -> Option<RgbImage> {
        self.frame.clone()
    }
    fn supports_manual_exposure(&self) -> bool {
        false
    }
}

struct TestFixture {
    users_dir: String,
    config_file: String,
    pub auth_engine: AuthEngine,
}

impl TestFixture {
    fn new(test_name: &str) -> Self {
        let users_dir = format!("/tmp/linuxcampam_test_users_{test_name}");
        let config_file = format!("/tmp/test_auth_config_{test_name}.ini");

        let _ = fs::create_dir_all(&users_dir);

        {
            let mut f = File::create(&config_file).unwrap();
            writeln!(f, "[Paths]").unwrap();
            writeln!(f, "users_dir={users_dir}").unwrap();
            writeln!(f).unwrap();
            writeln!(f, "[Cameras]").unwrap();
            writeln!(f, "names=mock_cam").unwrap();
            writeln!(f).unwrap();
            writeln!(f, "[Camera.mock_cam]").unwrap();
            writeln!(f, "path=/dev/video0").unwrap();
            writeln!(f, "type=rgb").unwrap();
            writeln!(f).unwrap();
            writeln!(f, "[Security]").unwrap();
            writeln!(f, "lockout_attempts=3").unwrap();
            writeln!(f, "lockout_duration_sec=10").unwrap();
        }

        Self {
            users_dir,
            config_file,
            auth_engine: AuthEngine::new(),
        }
    }

    fn init_with_mock_camera(&mut self) {
        self.auth_engine.set_camera_factory(|_def: &CameraDefinition| {
            Box::new(MockCamera { frame: None })
        });
        assert!(self.auth_engine.init(&self.config_file));
    }

    fn make_dummy_vec() -> Vec<f32> {
        vec![0.1f32; MOCK_EMBEDDING_SIZE]
    }

    fn write_user_with_embedding(&self, username: &str, label: &str) {
        let user_path = format!("{}/{username}.json", self.users_dir);
        let val = json!({
            "embeddings_rgb": [
                {
                    "label": label,
                    "data": Self::make_dummy_vec()
                }
            ]
        });
        fs::write(user_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.config_file);
        let _ = fs::remove_dir_all(&self.users_dir);
    }
}

#[test]
fn test_initialization_loads_cameras() {
    let mut fix = TestFixture::new("init_loads");
    let factory_called = Arc::new(AtomicBool::new(false));
    let factory_called_clone = Arc::clone(&factory_called);

    fix.auth_engine.set_camera_factory(move |def: &CameraDefinition| {
        factory_called_clone.store(true, Ordering::SeqCst);
        assert_eq!(def.id, "mock_cam");
        Box::new(MockCamera { frame: None })
    });

    assert!(fix.auth_engine.init(&fix.config_file));
    assert!(factory_called.load(Ordering::SeqCst), "Camera factory should have been called during init");
}

#[test]
fn test_verify_user_returns_false_when_not_enrolled() {
    let mut fix = TestFixture::new("not_enrolled");
    fix.init_with_mock_camera();

    let result = fix.auth_engine.verify_user_with_details("ghost_user");
    assert!(!result.success);
    assert!(!result.reason.is_empty());
}

#[test]
fn test_rate_limiting_lockout() {
    let mut fix = TestFixture::new("rate_limit");
    fix.auth_engine.set_camera_factory(|_def: &CameraDefinition| {
        let black_frame = RgbImage::new(MOCK_FRAME_WIDTH, MOCK_FRAME_HEIGHT);
        Box::new(MockCamera { frame: Some(black_frame) })
    });
    assert!(fix.auth_engine.init(&fix.config_file));

    fix.write_user_with_embedding("user_limited", "default");

    for _ in 0..3 {
        let res = fix.auth_engine.verify_user_with_details("user_limited");
        assert!(!res.success);
    }

    let lockout_res = fix.auth_engine.verify_user_with_details("user_limited");
    assert!(!lockout_res.success);
    assert!(lockout_res.reason.to_lowercase().contains("locked"));
}

#[test]
fn test_handles_empty_frames_gracefully() {
    let mut fix = TestFixture::new("empty_frames");
    fix.auth_engine.set_camera_factory(|_def: &CameraDefinition| {
        Box::new(MockCamera { frame: None })
    });
    assert!(fix.auth_engine.init(&fix.config_file));

    fix.write_user_with_embedding("user_empty_frame", "default");

    let res = fix.auth_engine.verify_user_with_details("user_empty_frame");
    assert!(!res.success);
}

#[test]
fn test_resilient_to_bad_model_paths() {
    let config_path = "/tmp/test_bad_models.ini";
    {
        let mut f = File::create(config_path).unwrap();
        writeln!(f, "[Paths]").unwrap();
        writeln!(f, "users_dir=/tmp/linuxcampam_test_users").unwrap();
        writeln!(f, "detection_model=/tmp/missing_det.onnx").unwrap();
        writeln!(f, "recognition_model=/tmp/missing_rec.onnx").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Cameras]").unwrap();
        writeln!(f, "names=mock_cam").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Camera.mock_cam]").unwrap();
        writeln!(f, "path=/dev/video0").unwrap();
        writeln!(f, "type=rgb").unwrap();
    }

    let mut engine = AuthEngine::new();
    let init_ok = engine.init(config_path);
    if init_ok {
        let res = engine.verify_user_with_details("any_user");
        assert!(!res.success);
    }

    let _ = fs::remove_file(config_path);
}

#[test]
fn test_set_label_promotes_pending_embedding() {
    let mut fix = TestFixture::new("set_label_promotes");
    fix.init_with_mock_camera();

    let user_path = format!("{}/test_user.json", fix.users_dir);
    let val = json!({
        "_pending_rgb": TestFixture::make_dummy_vec()
    });
    fs::write(&user_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

    assert!(fix.auth_engine.set_label("test_user", "my_label"));

    let content = fs::read_to_string(&user_path).unwrap();
    let j: Value = serde_json::from_str(&content).unwrap();
    assert!(!j.as_object().unwrap().contains_key("_pending_rgb"));
    assert!(j.as_object().unwrap().contains_key("embeddings_rgb"));
    let arr = j["embeddings_rgb"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["label"].as_str().unwrap(), "my_label");

    let stored = arr[0]["data"].as_array().unwrap();
    assert_eq!(stored.len(), MOCK_EMBEDDING_SIZE);
    for item in stored {
        assert!((item.as_f64().unwrap() as f32 - 0.1f32).abs() < 1e-5);
    }
}

#[test]
fn test_set_label_returns_false_without_pending_data() {
    let mut fix = TestFixture::new("set_label_no_pending");
    fix.init_with_mock_camera();

    fix.write_user_with_embedding("test_user", "existing");
    assert!(!fix.auth_engine.set_label("test_user", "new_label"));
}

#[test]
fn test_remove_embedding_deletes_correct_label() {
    let mut fix = TestFixture::new("remove_emb_correct");
    fix.init_with_mock_camera();

    let user_path = format!("{}/test_user.json", fix.users_dir);
    let val = json!({
        "embeddings_rgb": [
            { "label": "label_to_keep", "data": TestFixture::make_dummy_vec() },
            { "label": "label_to_remove", "data": TestFixture::make_dummy_vec() }
        ]
    });
    fs::write(&user_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

    assert!(fix.auth_engine.remove_embedding("test_user", "label_to_remove"));

    let content = fs::read_to_string(&user_path).unwrap();
    let j: Value = serde_json::from_str(&content).unwrap();
    let arr = j["embeddings_rgb"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["label"].as_str().unwrap(), "label_to_keep");
}

#[test]
fn test_remove_embedding_returns_false_for_nonexistent_label() {
    let mut fix = TestFixture::new("remove_emb_nonexist");
    fix.init_with_mock_camera();

    fix.write_user_with_embedding("test_user", "existing_label");
    assert!(!fix.auth_engine.remove_embedding("test_user", "no_such_label"));
}

#[test]
fn test_remove_embedding_returns_false_for_missing_user() {
    let mut fix = TestFixture::new("remove_emb_missing");
    fix.init_with_mock_camera();

    assert!(!fix.auth_engine.remove_embedding("nonexistent_user", "any_label"));
}

#[test]
fn test_train_user_handles_missing_face() {
    let mut fix = TestFixture::new("train_missing_face");
    fix.auth_engine.set_camera_factory(|_def: &CameraDefinition| {
        Box::new(MockCamera { frame: None })
    });
    assert!(fix.auth_engine.init(&fix.config_file));

    let user_path = format!("{}/test_user.json", fix.users_dir);
    fs::write(user_path, "{}").unwrap();

    assert!(!fix.auth_engine.train_user("test_user", "new_label", true));
}

#[test]
fn test_train_user_rejects_invalid_username() {
    let mut fix = TestFixture::new("train_invalid_user");
    fix.init_with_mock_camera();

    assert!(!fix.auth_engine.train_user("../evil", "label", true));
    assert!(!fix.auth_engine.train_user("", "label", false));
}

#[test]
fn test_valid_frame_triggers_face_detection() {
    let mut fix = TestFixture::new("valid_frame");
    fix.auth_engine.set_camera_factory(|_def: &CameraDefinition| {
        let grey_frame = RgbImage::from_pixel(
            MOCK_FRAME_WIDTH,
            MOCK_FRAME_HEIGHT,
            Rgb([128, 128, 128]),
        );
        Box::new(MockCamera { frame: Some(grey_frame) })
    });
    assert!(fix.auth_engine.init(&fix.config_file));

    fix.write_user_with_embedding("user_valid_frame", "default");

    let res = fix.auth_engine.verify_user_with_details("user_valid_frame");
    assert!(!res.success);
    assert!(res.reason.contains("No face detected"));
}

#[test]
fn test_lockout_activates_after_n_failures() {
    let mut fix = TestFixture::new("lockout_n_failures");
    fix.init_with_mock_camera();

    assert!(!fix.auth_engine.is_user_locked_out("alice"));
    fix.auth_engine.record_auth_attempt("alice", false);
    fix.auth_engine.record_auth_attempt("alice", false);
    assert!(!fix.auth_engine.is_user_locked_out("alice"));
    fix.auth_engine.record_auth_attempt("alice", false);
    assert!(fix.auth_engine.is_user_locked_out("alice"));
}

#[test]
fn test_lockout_blocks_verify_without_models() {
    let mut fix = TestFixture::new("lockout_blocks_verify");
    fix.init_with_mock_camera();

    fix.auth_engine.record_auth_attempt("bob", false);
    fix.auth_engine.record_auth_attempt("bob", false);
    fix.auth_engine.record_auth_attempt("bob", false);
    assert!(fix.auth_engine.is_user_locked_out("bob"));

    let res = fix.auth_engine.verify_user_with_details("bob");
    assert!(!res.success);
    assert!(res.reason.to_lowercase().contains("locked"));
}

#[test]
fn test_successful_attempt_resets_counter() {
    let mut fix = TestFixture::new("reset_counter");
    fix.init_with_mock_camera();

    fix.auth_engine.record_auth_attempt("charlie", false);
    fix.auth_engine.record_auth_attempt("charlie", false);
    assert!(!fix.auth_engine.is_user_locked_out("charlie"));

    fix.auth_engine.record_auth_attempt("charlie", true);
    assert!(!fix.auth_engine.is_user_locked_out("charlie"));

    fix.auth_engine.record_auth_attempt("charlie", false);
    fix.auth_engine.record_auth_attempt("charlie", false);
    assert!(!fix.auth_engine.is_user_locked_out("charlie"));
}

#[test]
fn test_lockout_disabled_when_attempts_is_zero() {
    let config_path = "/tmp/test_no_lockout.ini";
    let users_dir = "/tmp/linuxcampam_test_users_no_lockout";
    let _ = fs::create_dir_all(users_dir);

    {
        let mut f = File::create(config_path).unwrap();
        writeln!(f, "[Paths]").unwrap();
        writeln!(f, "users_dir={users_dir}").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Cameras]").unwrap();
        writeln!(f, "names=mock_cam").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Camera.mock_cam]").unwrap();
        writeln!(f, "path=/dev/video0").unwrap();
        writeln!(f, "type=rgb").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Security]").unwrap();
        writeln!(f, "lockout_attempts=0").unwrap();
        writeln!(f, "lockout_duration_sec=10").unwrap();
    }

    let mut engine = AuthEngine::new();
    engine.set_camera_factory(|_def: &CameraDefinition| {
        Box::new(MockCamera { frame: None })
    });
    assert!(engine.init(config_path));

    for _ in 0..100 {
        engine.record_auth_attempt("dave", false);
    }
    assert!(!engine.is_user_locked_out("dave"));

    let _ = fs::remove_file(config_path);
    let _ = fs::remove_dir_all(users_dir);
}

#[test]
fn test_lockout_expires_after_duration() {
    let config_path = "/tmp/test_short_lockout.ini";
    let users_dir = "/tmp/linuxcampam_test_users_short_lockout";
    let _ = fs::create_dir_all(users_dir);

    {
        let mut f = File::create(config_path).unwrap();
        writeln!(f, "[Paths]").unwrap();
        writeln!(f, "users_dir={users_dir}").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Cameras]").unwrap();
        writeln!(f, "names=mock_cam").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Camera.mock_cam]").unwrap();
        writeln!(f, "path=/dev/video0").unwrap();
        writeln!(f, "type=rgb").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[Security]").unwrap();
        writeln!(f, "lockout_attempts=3").unwrap();
        writeln!(f, "lockout_duration_sec=1").unwrap();
    }

    let mut engine = AuthEngine::new();
    engine.set_camera_factory(|_def: &CameraDefinition| {
        Box::new(MockCamera { frame: None })
    });
    assert!(engine.init(config_path));

    engine.record_auth_attempt("eve", false);
    engine.record_auth_attempt("eve", false);
    engine.record_auth_attempt("eve", false);
    assert!(engine.is_user_locked_out("eve"));

    thread::sleep(Duration::from_secs(2));
    assert!(!engine.is_user_locked_out("eve"));

    let _ = fs::remove_file(config_path);
    let _ = fs::remove_dir_all(users_dir);
}

#[test]
fn test_lockout_is_user_specific() {
    let mut fix = TestFixture::new("user_specific");
    fix.init_with_mock_camera();

    fix.auth_engine.record_auth_attempt("frank", false);
    fix.auth_engine.record_auth_attempt("frank", false);
    fix.auth_engine.record_auth_attempt("frank", false);
    assert!(fix.auth_engine.is_user_locked_out("frank"));

    assert!(!fix.auth_engine.is_user_locked_out("grace"));
}

#[test]
fn test_set_label_writes_file_with_secure_permissions() {
    let mut fix = TestFixture::new("secure_perms");
    fix.init_with_mock_camera();

    let user_path = format!("{}/perm_user.json", fix.users_dir);
    let val = json!({
        "_pending_rgb": TestFixture::make_dummy_vec()
    });
    fs::write(&user_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

    assert!(fix.auth_engine.set_label("perm_user", "perm_label"));

    let meta = fs::metadata(&user_path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o777,
        0o600,
        "setLabel must produce a 0600 file"
    );

    let dir_meta = fs::metadata(&fix.users_dir).unwrap();
    assert_eq!(
        dir_meta.permissions().mode() & 0o777,
        0o700,
        "writeJsonAtomic must tighten the users directory to 0700"
    );

    for entry in fs::read_dir(&fix.users_dir).unwrap() {
        let entry = entry.unwrap();
        let fname = entry.file_name().to_string_lossy().to_string();
        assert!(
            !fname.contains(".tmp."),
            "leftover temp file: {}",
            entry.path().display()
        );
    }
}
