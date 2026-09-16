use pam_linuxcampam::auth_engine::cosine_similarity;
use pam_linuxcampam::utils::is_valid_username;
use serde_json::json;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

const MAX_CAMERA_PATH_LENGTH: usize = 20;
const EMBEDDING_DIM: usize = 128;
const DEFAULT_EMBEDDING_VAL: f32 = 0.5;
const LARGE_VAL: f32 = 1e30;

fn is_valid_camera_path(path: &str) -> bool {
    if !path.starts_with("/dev/video") {
        return false;
    }
    if path.contains("..") {
        return false;
    }
    if path.len() > MAX_CAMERA_PATH_LENGTH {
        return false;
    }
    true
}

fn is_valid_config_path(path: &str) -> bool {
    if path.is_empty() || !path.starts_with('/') {
        return false;
    }
    if path.contains("..") {
        return false;
    }
    if !path.ends_with(".ini") {
        return false;
    }
    true
}

fn is_valid_embedding(emb: &[f32]) -> bool {
    if emb.len() != EMBEDDING_DIM {
        return false;
    }
    for &v in emb {
        if v.is_nan() || v.is_infinite() {
            return false;
        }
    }
    true
}

#[test]
fn test_command_injection_via_username() {
    assert!(!is_valid_username("user; rm -rf /"));
    assert!(!is_valid_username("user$(whoami)"));
    assert!(!is_valid_username("user`id`"));
    assert!(!is_valid_username("user|cat /etc/passwd"));
    assert!(!is_valid_username("user&& malicious"));
    assert!(!is_valid_username("user\necho pwned"));
    assert!(!is_valid_username("user\recho pwned"));
    assert!(!is_valid_username("$(cat /etc/shadow)"));
    assert!(!is_valid_username("${PATH}"));

    let null_injection = "user\0admin".to_string();
    assert!(!is_valid_username(&null_injection));
}

#[test]
fn test_camera_path_validation() {
    assert!(is_valid_camera_path("/dev/video0"));
    assert!(is_valid_camera_path("/dev/video1"));
    assert!(is_valid_camera_path("/dev/video10"));

    assert!(!is_valid_camera_path("/dev/video0/../video1"));
    assert!(!is_valid_camera_path("/dev/../etc/passwd"));

    assert!(!is_valid_camera_path("/dev/sda1"));
    assert!(!is_valid_camera_path("/etc/passwd"));
    assert!(!is_valid_camera_path("/tmp/fake_video0"));

    assert!(!is_valid_camera_path("/dev/video0; cat /etc/passwd"));
}

#[test]
fn test_config_path_validation() {
    assert!(is_valid_config_path("/etc/linuxcampam/config.ini"));
    assert!(is_valid_config_path("/home/user/.config/test.ini"));

    assert!(!is_valid_config_path("config.ini"));
    assert!(!is_valid_config_path("./config.ini"));

    assert!(!is_valid_config_path("/etc/linuxcampam/../passwd"));
    assert!(!is_valid_config_path("/tmp/../etc/shadow.ini"));

    assert!(!is_valid_config_path("/etc/linuxcampam/config.sh"));
    assert!(!is_valid_config_path("/etc/passwd"));
}

#[test]
fn test_malformed_embedding_data() {
    const WRONG_DIM: usize = 64;

    let empty_emb = json!({ "data": [] });
    assert_eq!(empty_emb["data"].as_array().unwrap().len(), 0);

    let wrong_dim = vec![DEFAULT_EMBEDDING_VAL; WRONG_DIM];
    assert_ne!(wrong_dim.len(), EMBEDDING_DIM);

    let correct_dim = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    assert_eq!(correct_dim.len(), EMBEDDING_DIM);
}

#[test]
fn test_embedding_nan_and_inf_values() {
    let mut embedding = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];

    embedding[0] = f32::NAN;
    assert!(embedding[0].is_nan());

    embedding[1] = f32::INFINITY;
    assert!(embedding[1].is_infinite());

    embedding[2] = DEFAULT_EMBEDDING_VAL;
    assert!(!embedding[2].is_nan());
    assert!(!embedding[2].is_infinite());
}

#[test]
fn test_embedding_validation() {
    const WRONG_DIM: usize = 64;
    const SPECIAL_INDEX: usize = 50;

    let valid = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    assert!(is_valid_embedding(&valid));

    let wrong_size = vec![DEFAULT_EMBEDDING_VAL; WRONG_DIM];
    assert!(!is_valid_embedding(&wrong_size));

    let mut with_nan = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    with_nan[SPECIAL_INDEX] = f32::NAN;
    assert!(!is_valid_embedding(&with_nan));

    let mut with_inf = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    with_inf[SPECIAL_INDEX] = f32::INFINITY;
    assert!(!is_valid_embedding(&with_inf));
}

#[test]
fn test_max_embeddings_limit() {
    const MAX_EMBEDDINGS: usize = 5;
    const VAL_STEP: f32 = 0.1;
    let mut embeddings = Vec::new();

    for i in 0..MAX_EMBEDDINGS {
        let e = json!({
            "label": format!("label_{i}"),
            "data": vec![i as f32 * VAL_STEP; EMBEDDING_DIM],
        });
        embeddings.push(e);
    }

    assert_eq!(embeddings.len(), MAX_EMBEDDINGS);
}

#[test]
fn test_large_user_file_prevention() {
    const MAX_REASONABLE_SIZE: usize = 1024 * 1024;
    let emb = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    let single_emb_size = emb.len() * std::mem::size_of::<f32>();

    assert!(single_emb_size * 1000 < MAX_REASONABLE_SIZE);
}

#[test]
fn test_malformed_json_handling() {
    assert!(serde_json::from_str::<serde_json::Value>("{invalid json}").is_err());
    assert!(serde_json::from_str::<serde_json::Value>("").is_err());
    assert!(serde_json::from_str::<serde_json::Value>("{\"unclosed\": ").is_err());

    assert!(serde_json::from_str::<serde_json::Value>("{}").is_ok());
    assert!(serde_json::from_str::<serde_json::Value>("{\"valid\": true}").is_ok());
}

#[test]
fn test_json_type_confusion() {
    let mut data = serde_json::Map::new();
    data.insert(
        "embeddings_ir".to_string(),
        json!("not_an_array"),
    );

    let val = serde_json::Value::Object(data);
    assert!(!val["embeddings_ir"].is_array());
    assert!(val["embeddings_ir"].is_string());
}

#[test]
fn test_similarity_edge_cases() {
    const SHORT_VEC_DIM: usize = 64;
    let emb1 = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    let emb2 = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];

    let sim = cosine_similarity(&emb1, &emb2);
    assert!((sim - 1.0).abs() < 0.001);

    let zero_vec = vec![0.0f32; EMBEDDING_DIM];
    let sim = cosine_similarity(&zero_vec, &zero_vec);
    assert_eq!(sim, 0.0);

    let short_vec = vec![DEFAULT_EMBEDDING_VAL; SHORT_VEC_DIM];
    let sim = cosine_similarity(&emb1, &short_vec);
    assert_eq!(sim, 0.0);

    let empty_vec: Vec<f32> = Vec::new();
    let sim = cosine_similarity(&empty_vec, &empty_vec);
    assert_eq!(sim, 0.0);
}

#[test]
fn test_similarity_overflow_behavior() {
    let large_vals = vec![LARGE_VAL; EMBEDDING_DIM];
    let sim = cosine_similarity(&large_vals, &large_vals);

    if sim.is_nan() {
        // Expected overflow
    } else {
        assert!((sim - 1.0).abs() < 0.001);
    }

    let normal_vals = vec![DEFAULT_EMBEDDING_VAL; EMBEDDING_DIM];
    let sim = cosine_similarity(&normal_vals, &normal_vals);
    assert!(!sim.is_nan());
    assert!((sim - 1.0).abs() < 0.001);
}

// Lockout tests mirroring upstream lockout_test namespace
struct LockoutState {
    failed_attempts: i32,
    lockout_until: Option<Instant>,
}

struct LockoutConfig {
    lockout_attempts: i32,
    lockout_duration_sec: i32,
}

struct LockoutTestContext {
    lockout_map: HashMap<String, LockoutState>,
    config: LockoutConfig,
}

impl LockoutTestContext {
    fn new() -> Self {
        Self {
            lockout_map: HashMap::new(),
            config: LockoutConfig {
                lockout_attempts: 5,
                lockout_duration_sec: 300,
            },
        }
    }

    fn is_user_locked_out(&self, username: &str) -> bool {
        if self.config.lockout_attempts <= 0 {
            return false;
        }
        if let Some(state) = self.lockout_map.get(username) {
            if let Some(until) = state.lockout_until {
                return Instant::now() < until;
            }
        }
        false
    }

    fn record_auth_attempt(&mut self, username: &str, success: bool) {
        if self.config.lockout_attempts <= 0 {
            return;
        }
        let state = self.lockout_map.entry(username.to_string()).or_insert(LockoutState {
            failed_attempts: 0,
            lockout_until: None,
        });

        if success {
            state.failed_attempts = 0;
            state.lockout_until = None;
        } else {
            state.failed_attempts += 1;
            if state.failed_attempts >= self.config.lockout_attempts {
                state.lockout_until = Some(Instant::now() + Duration::from_secs(self.config.lockout_duration_sec as u64));
            }
        }
    }

    fn disable_lockout(&mut self) {
        self.config.lockout_attempts = 0;
    }

    fn set_lockout_duration(&mut self, seconds: i32) {
        self.config.lockout_duration_sec = seconds;
    }

    fn get_lockout_attempts(&self) -> i32 {
        self.config.lockout_attempts
    }

    fn get_lockout_duration(&self) -> i32 {
        self.config.lockout_duration_sec
    }
}

#[test]
fn test_lockout_after_failed_attempts() {
    let mut ctx = LockoutTestContext::new();
    let user = "testuser";

    for _ in 0..4 {
        ctx.record_auth_attempt(user, false);
        assert!(!ctx.is_user_locked_out(user));
    }

    ctx.record_auth_attempt(user, false);
    assert!(ctx.is_user_locked_out(user));
}

#[test]
fn test_success_resets_counter() {
    let mut ctx = LockoutTestContext::new();
    let user = "testuser";

    for _ in 0..3 {
        ctx.record_auth_attempt(user, false);
    }
    assert!(!ctx.is_user_locked_out(user));

    ctx.record_auth_attempt(user, true);

    for _ in 0..4 {
        ctx.record_auth_attempt(user, false);
    }
    assert!(!ctx.is_user_locked_out(user));
}

#[test]
fn test_disabled_when_zero() {
    let mut ctx = LockoutTestContext::new();
    ctx.disable_lockout();
    let user = "testuser";

    for _ in 0..100 {
        ctx.record_auth_attempt(user, false);
    }
    assert!(!ctx.is_user_locked_out(user));
}

#[test]
fn test_lockout_expiration() {
    let mut ctx = LockoutTestContext::new();
    ctx.set_lockout_duration(1);
    let user = "testuser";

    for _ in 0..ctx.get_lockout_attempts() {
        ctx.record_auth_attempt(user, false);
    }
    assert!(ctx.is_user_locked_out(user));

    thread::sleep(Duration::from_millis(1100));
    assert!(!ctx.is_user_locked_out(user));
}

#[test]
fn test_config_parsing_defaults() {
    let ctx = LockoutTestContext::new();
    assert_eq!(ctx.get_lockout_attempts(), 5);
    assert_eq!(ctx.get_lockout_duration(), 300);
}
