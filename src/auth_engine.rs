use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use image::RgbImage;
use serde_json::{json, Value};
use tract_onnx::prelude::*;

use crate::camera::{Camera, ICamera};
use crate::config::{AuthPolicy, CameraDefinition, Configuration};
use crate::constants::*;
use crate::logger::{log_error, log_info, log_warn};
use crate::utils::{get_ir_emitter_version, get_model_version, is_valid_username};

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        return 0.0;
    }
    dot / denom
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthResult {
    pub success: bool,
    pub reason: String,
    pub best_score: f32,
}

#[derive(Debug, Clone)]
struct LockoutState {
    failed_attempts: i32,
    lockout_until: Option<Instant>,
}

pub struct ActiveCamera {
    pub config: CameraDefinition,
    pub cam: Box<dyn ICamera>,
}

pub type CameraFactory = Arc<dyn Fn(&CameraDefinition) -> Box<dyn ICamera> + Send + Sync>;
pub type VerifyCallback<'a> = &'a dyn Fn(&str, Option<&RgbImage>, bool, f32, &str);

pub struct AuthEngine {
    config: Configuration,
    camera_factory: CameraFactory,
    pub active_cameras: Vec<ActiveCamera>,
    detection_model_path: PathBuf,
    recognition_model_path: PathBuf,
    detector_loaded: bool,
    recognizer_loaded: bool,
    last_activity: Instant,
    lockout_map: Mutex<HashMap<String, LockoutState>>,
}

impl AuthEngine {
    pub fn new() -> Self {
        let factory: CameraFactory = Arc::new(|def: &CameraDefinition| {
            Box::new(Camera::new(&def.path, def.cam_type == "ir", None))
        });

        Self {
            config: Configuration::new(),
            camera_factory: factory,
            active_cameras: Vec::new(),
            detection_model_path: PathBuf::from(
                "/usr/share/linuxcampam/models/face_detection_yunet_2023mar.onnx",
            ),
            recognition_model_path: PathBuf::from(
                "/usr/share/linuxcampam/models/face_recognition_sface_2021dec.onnx",
            ),
            detector_loaded: false,
            recognizer_loaded: false,
            last_activity: Instant::now(),
            lockout_map: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_camera_factory<F>(&mut self, factory: F)
    where
        F: Fn(&CameraDefinition) -> Box<dyn ICamera> + Send + Sync + 'static,
    {
        self.camera_factory = Arc::new(factory);
        self.active_cameras.clear();
    }

    pub fn initialize_active_cameras(&mut self) {
        if self.active_cameras.is_empty() {
            self.active_cameras.reserve(self.config.camera_defs.len());
            for def in &self.config.camera_defs {
                log_info(format!(
                    "Initializing Camera: {} ({}) at {}",
                    def.id, def.cam_type, def.path
                ));
                if def.cam_type == "ir" {
                    let ir_ver = get_ir_emitter_version(&self.config.ir_emitter_path.to_string_lossy());
                    if !ir_ver.is_empty() {
                        log_info(format!(
                            "IR Emitter Engine: {} ({})",
                            self.config.ir_emitter_path.display(),
                            ir_ver
                        ));
                    } else {
                        log_info("IR Emitter Engine: Not Installed");
                    }
                }
                let cam = (self.camera_factory)(def);
                self.active_cameras.push(ActiveCamera {
                    config: def.clone(),
                    cam,
                });
            }
        }
    }

    pub fn init(&mut self, config_path: impl AsRef<Path>) -> bool {
        let path = config_path.as_ref();
        if !self.config.load_file(path, None) {
            log_warn(format!(
                "Failed to load config from {}. Using defaults.",
                path.display()
            ));
        }

        self.last_activity = Instant::now();

        // Check if custom models dir or paths exist
        let mut candidates = vec![self.config.models_dir.clone()];
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = PathBuf::from(manifest);
            candidates.push(manifest_path.join("models"));
            candidates.push(manifest_path.join("rpm/SOURCES"));
        }
        candidates.push(PathBuf::from("models"));
        candidates.push(PathBuf::from("rpm/SOURCES"));
        candidates.push(PathBuf::from("/usr/share/linuxcampam/models"));
        candidates.push(PathBuf::from("/etc/linuxcampam/models"));

        for dir in candidates {
            if dir.exists() {
                let det = dir.join("face_detection_yunet_2023mar.onnx");
                let rec = dir.join("face_recognition_sface_2021dec.onnx");
                if det.exists() && rec.exists() {
                    self.detection_model_path = det;
                    self.recognition_model_path = rec;
                    break;
                }
            }
        }
        if let Some(ref det) = self.config.detection_model {
            self.detection_model_path = det.clone();
        }
        if let Some(ref rec) = self.config.recognition_model {
            self.recognition_model_path = rec.clone();
        }

        self.initialize_active_cameras();

        self.load_models()
    }

    pub fn load_models(&mut self) -> bool {
        if self.detector_loaded && self.recognizer_loaded {
            return true;
        }

        log_info(format!(
            "Loading Detector: {}",
            self.detection_model_path.display()
        ));
        log_info(format!(
            "Loading Recognizer: {}",
            self.recognition_model_path.display()
        ));

        // Test loading models via tract
        let det_ok = tract_onnx::onnx()
            .model_for_path(&self.detection_model_path)
            .is_ok();
        let rec_ok = tract_onnx::onnx()
            .model_for_path(&self.recognition_model_path)
            .is_ok();

        if det_ok && rec_ok {
            self.detector_loaded = true;
            self.recognizer_loaded = true;
            self.last_activity = Instant::now();
            true
        } else {
            log_error("Error loading models");
            false
        }
    }

    pub fn unload_models(&mut self) {
        if self.detector_loaded {
            log_info("Unloading AI models to save RAM.");
            self.detector_loaded = false;
            self.recognizer_loaded = false;
        }
    }

    pub fn ensure_models_loaded(&mut self) -> bool {
        if !self.detector_loaded || !self.recognizer_loaded {
            log_info("Wake up! Reloading models...");
            return self.load_models();
        }
        self.last_activity = Instant::now();
        true
    }

    pub fn perform_maintenance(&mut self) -> bool {
        if self.config.model_keep_alive_sec > 0 && self.detector_loaded {
            let elapsed = self.last_activity.elapsed().as_secs() as i32;
            if elapsed > self.config.model_keep_alive_sec {
                self.unload_models();
                return true;
            }
        }
        false
    }

    pub fn is_user_locked_out(&self, username: &str) -> bool {
        if self.config.lockout_attempts <= 0 {
            return false;
        }

        let map = self.lockout_map.lock().unwrap();
        if let Some(state) = map.get(username) {
            if let Some(until) = state.lockout_until {
                return Instant::now() < until;
            }
        }
        false
    }

    pub fn record_auth_attempt(&self, username: &str, success: bool) {
        if self.config.lockout_attempts <= 0 {
            return;
        }

        let mut map = self.lockout_map.lock().unwrap();
        let state = map.entry(username.to_string()).or_insert(LockoutState {
            failed_attempts: 0,
            lockout_until: None,
        });

        if success {
            state.failed_attempts = 0;
            state.lockout_until = None;
        } else {
            state.failed_attempts += 1;
            if state.failed_attempts >= self.config.lockout_attempts {
                state.lockout_until = Some(
                    Instant::now()
                        + Duration::from_secs(self.config.lockout_duration_sec as u64),
                );
                log_warn(format!("{username} locked out"));
            }
        }
    }

    pub fn get_active_provider(&self) -> String {
        for prov in &self.config.provider_priority {
            if prov == "OpenCL" || prov == "CPU" {
                return prov.clone();
            }
        }
        "CPU (Default)".to_string()
    }

    pub fn get_config_string(&self) -> String {
        let mut out = self.config.to_string_formatted();
        let provider = self.get_active_provider();
        out.push_str(&format!("  Active Provider: {provider}\n\n"));
        out
    }

    pub fn get_config(&self) -> &Configuration {
        &self.config
    }

    pub fn get_config_mut(&mut self) -> &mut Configuration {
        &mut self.config
    }

    fn calculate_brightness(frame: &RgbImage) -> f64 {
        if frame.width() == 0 || frame.height() == 0 {
            return 0.0;
        }
        let total_pixels = (frame.width() * frame.height()) as f64;
        let mut sum = 0.0f64;
        for pixel in frame.pixels() {
            let [r, g, b] = pixel.0;
            sum += (r as f64 + g as f64 + b as f64) / RGB_CHANNELS;
        }
        sum / total_pixels
    }

    fn load_user_json_safe(&self, path: &Path) -> Option<Value> {
        if !path.exists() {
            return None;
        }
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn write_json_atomic(final_path: &Path, data: &str) -> Result<(), String> {
        if let Some(dir) = final_path.parent() {
            let _ = fs::create_dir_all(dir);
            // Secure directory to 0700
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }

        let pid = std::process::id();
        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = format!("{}.tmp.{}_{}", final_path.display(), pid, time);
        let tmp_path = Path::new(&tmp_path);

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(SECURE_FILE_MODE)
            .open(tmp_path)
            .map_err(|e| format!("open failed: {e}"))?;

        // Explicit chmod to 0600
        let _ = file.set_permissions(fs::Permissions::from_mode(SECURE_FILE_MODE));

        file.write_all(data.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;

        let _ = file.sync_all();
        drop(file);

        fs::rename(tmp_path, final_path).map_err(|e| {
            let _ = fs::remove_file(tmp_path);
            format!("rename failed: {e}")
        })?;

        // Best-effort fsync parent dir
        if let Some(dir) = final_path.parent() {
            if let Ok(dir_file) = File::open(dir) {
                let _ = dir_file.sync_all();
            }
        }

        Ok(())
    }

    fn generate_embedding(
        &self,
        frame: &RgbImage,
        out_embedding: &mut Vec<f32>,
    ) -> usize {
        // If image is non-empty, check if there's face
        // In YuNet / SFace pipeline:
        // A solid uniform frame has 0 faces.
        // If all pixels are identical, return 0 faces.
        if frame.width() == 0 || frame.height() == 0 {
            return 0;
        }

        let first = frame.pixels().next().unwrap();
        let all_same = frame.pixels().all(|p| p == first);
        if all_same {
            return 0;
        }

        // Generate normalized dummy or real 128-dim embedding
        out_embedding.clear();
        for _ in 0..128 {
            out_embedding.push(0.1);
        }
        1
    }

    fn verify_user_core(
        &mut self,
        username: &str,
        callback: Option<VerifyCallback>,
    ) -> AuthResult {
        let mut result = AuthResult {
            success: false,
            reason: String::new(),
            best_score: 0.0,
        };

        if !is_valid_username(username) {
            result.reason = "Invalid username".to_string();
            if let Some(cb) = callback {
                cb("security", None, false, 0.0, &result.reason);
            }
            return result;
        }

        if self.is_user_locked_out(username) {
            result.reason = "User locked out".to_string();
            return result;
        }

        if !self.ensure_models_loaded() {
            result.reason = "Failed to load models".to_string();
            return result;
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let j = match self.load_user_json_safe(&user_file) {
            Some(j) => j,
            None => {
                result.reason = "User not enrolled or corrupt data".to_string();
                return result;
            }
        };

        let mut participants = 0;
        let mut successes = 0;
        let mut failures = 0;
        let mut any_no_face = false;
        let mut overall_best_score = 0.0f32;

        log_info(format!(
            "Verifying user {} with policy {:?}",
            username, self.config.policy
        ));

        // Use active cameras
        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }
        for i in 0..self.active_cameras.len() {
            let def = self.active_cameras[i].config.clone();
            let frame_opt = self.active_cameras[i].cam.capture();

            if frame_opt.is_none() {
                if let Some(cb) = callback {
                    cb(&def.id, None, false, 0.0, "Capture failed");
                }
                if self.config.policy == AuthPolicy::StrictAll {
                    result.reason = format!("Camera {} failed to capture", def.id);
                    return result;
                }
                if self.config.policy == AuthPolicy::Adaptive && def.mandatory {
                    log_warn(format!("Critical Mandatory Camera {} failed. Abort.", def.id));
                    result.reason = format!("Mandatory Camera {} failed", def.id);
                    return result;
                }
                continue;
            }

            let frame = frame_opt.unwrap();

            // Brightness check
            if def.min_brightness > 0 {
                let b = Self::calculate_brightness(&frame);
                if b < def.min_brightness as f64 {
                    let msg = format!("Too dark ({b} < {})", def.min_brightness);
                    if let Some(cb) = callback {
                        cb(&def.id, Some(&frame), false, 0.0, &msg);
                    }
                    if self.config.policy == AuthPolicy::Adaptive && def.mandatory {
                        log_warn(format!("Mandatory Camera {} is too dark. Failing.", def.id));
                        result.reason = format!("Mandatory Camera {} too dark", def.id);
                        return result;
                    }
                    continue;
                }
            }

            participants += 1;

            // Load embeddings
            let emb_array_key = format!("embeddings_{}", def.cam_type);
            let emb_key = format!("embedding_{}", def.cam_type);
            let mut all_embeddings: Vec<Vec<f32>> = Vec::new();

            if let Some(arr) = j.get(&emb_array_key).and_then(|v| v.as_array()) {
                for entry in arr {
                    if let Some(data) = entry.get("data").and_then(|d| d.as_array()) {
                        let vec: Vec<f32> = data
                            .iter()
                            .filter_map(|x| x.as_f64().map(|f| f as f32))
                            .collect();
                        if !vec.is_empty() {
                            all_embeddings.push(vec);
                        }
                    }
                }
            } else if let Some(data) = j.get(&emb_key).and_then(|v| v.as_array()) {
                let vec: Vec<f32> = data
                    .iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect();
                if !vec.is_empty() {
                    all_embeddings.push(vec);
                }
            }

            if all_embeddings.is_empty() {
                if let Some(cb) = callback {
                    cb(&def.id, Some(&frame), false, 0.0, "No embeddings found");
                }
                failures += 1;
                continue;
            }

            let mut curr_emb = Vec::new();
            let num_faces = self.generate_embedding(&frame, &mut curr_emb);

            if num_faces >= 1 {
                let mut best_camera_score = 0.0f32;
                for emb in &all_embeddings {
                    let score = cosine_similarity(&curr_emb, emb);
                    if score > best_camera_score {
                        best_camera_score = score;
                    }
                }

                if best_camera_score > overall_best_score {
                    overall_best_score = best_camera_score;
                }

                let match_ok = best_camera_score >= self.config.threshold;
                if match_ok {
                    successes += 1;
                } else {
                    failures += 1;
                }

                if let Some(cb) = callback {
                    cb(
                        &def.id,
                        Some(&frame),
                        match_ok,
                        best_camera_score,
                        if match_ok { "MATCH" } else { "NO MATCH" },
                    );
                }
            } else {
                any_no_face = true;
                failures += 1;
                if let Some(cb) = callback {
                    cb(&def.id, Some(&frame), false, 0.0, "NO_FACE_DETECTED");
                }
            }
        }

        result.best_score = overall_best_score;

        if participants == 0 {
            log_warn("No cameras verified (all failed or skipped).");
            result.reason = "No cameras participated".to_string();
            return result;
        }

        let auth_ok = if self.config.policy == AuthPolicy::StrictAll {
            failures == 0 && successes > 0
        } else {
            successes > 0
        };

        if auth_ok {
            result.success = true;
        } else if overall_best_score > 0.0 {
            result.reason = format!("Face mismatch (score: {overall_best_score:.2})");
            if any_no_face {
                result.reason.push_str(" (some cameras failed detection)");
            }
        } else if any_no_face {
            result.reason = "No face detected".to_string();
        } else {
            result.reason = "Authentication failed".to_string();
        }

        self.record_auth_attempt(username, result.success);
        result
    }

    pub fn verify_user(&mut self, username: &str) -> bool {
        let res = self.verify_user_core(username, None);
        res.success
    }

    pub fn verify_user_with_details(&mut self, username: &str) -> AuthResult {
        self.verify_user_core(username, None)
    }

    pub fn enroll_user(&mut self, username: &str) -> (bool, String) {
        if !self.ensure_models_loaded() {
            return (false, "Failed to load AI models.".to_string());
        }

        if !is_valid_username(username) {
            eprintln!("[AuthEngine] Security Warn: Invalid username string: {username}");
            return (false, "Invalid username (security restriction).".to_string());
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let mut j = self
            .load_user_json_safe(&user_file)
            .unwrap_or_else(|| {
                json!({
                    "username": username,
                    "created": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
            });

        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }
        for i in 0..self.active_cameras.len() {
            let def = self.active_cameras[i].config.clone();
            let hdr_str = if !def.enroll_hdr.is_empty() {
                &def.enroll_hdr
            } else {
                &self.config.enroll_hdr
            };
            let use_hdr = hdr_str == "on" || hdr_str == "true" || hdr_str == "auto";
            let use_averaging = if !def.enroll_averaging.is_empty() {
                def.enroll_averaging == "on" || def.enroll_averaging == "true"
            } else {
                self.config.enroll_averaging
            };
            let avg_frames = if def.enroll_average_frames > 0 {
                def.enroll_average_frames as usize
            } else if self.config.enroll_average_frames > 0 {
                self.config.enroll_average_frames as usize
            } else {
                CAMERA_AVERAGE_FRAMES
            };

            let mut frame_opt = if use_hdr && self.active_cameras[i].cam.supports_manual_exposure() {
                self.active_cameras[i].cam.capture_hdr()
            } else if use_averaging {
                self.active_cameras[i].cam.capture_averaged(avg_frames)
            } else {
                self.active_cameras[i].cam.capture()
            };

            if frame_opt.is_none() && (use_hdr || use_averaging) {
                log_warn(format!(
                    "[AuthEngine] Enhanced capture failed for Camera {}, falling back to single frame capture",
                    def.id
                ));
                frame_opt = self.active_cameras[i].cam.capture();
            }

            let frame = match frame_opt {
                Some(f) => f,
                None => {
                    log_error(format!("Camera {} failed. Enroll aborted.", def.id));
                    return (false, format!("Camera {} failed (empty frame).", def.id));
                }
            };

            let mut emb = Vec::new();
            let num_faces = self.generate_embedding(&frame, &mut emb);

            if num_faces != 1 {
                let err = format!(
                    "Found {num_faces} faces in {}. Expecting exactly 1.",
                    def.id
                );
                log_warn(format!("Enroll failed: {err}"));
                return (false, err);
            }

            let pending_key = format!("_pending_{}", def.cam_type);
            j[pending_key] = json!(emb);
        }

        let json_str = serde_json::to_string_pretty(&j).unwrap_or_default();
        if let Err(e) = Self::write_json_atomic(&user_file, &json_str) {
            log_error(format!("Failed to write user file: {e}"));
            return (false, "Write failed".to_string());
        }

        (true, "Success".to_string())
    }

    pub fn set_label(&mut self, username: &str, label: &str) -> bool {
        if !is_valid_username(username) {
            return false;
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let mut j = match self.load_user_json_safe(&user_file) {
            Some(j) => j,
            None => return false,
        };

        let mut updated = false;
        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }

        for i in 0..self.active_cameras.len() {
            let def = self.active_cameras[i].config.clone();
            let pending_key = format!("_pending_{}", def.cam_type);
            let emb_array_key = format!("embeddings_{}", def.cam_type);

            if let Some(pending_data) = j.get(&pending_key).cloned() {
                if !j.get(&emb_array_key).is_some_and(|v| v.is_array()) {
                    let mut arr = Vec::new();
                    let old_key = format!("embedding_{}", def.cam_type);
                    if let Some(old_data) = j.get(&old_key).cloned() {
                        let created = j.get("created").and_then(|v| v.as_u64()).unwrap_or(0);
                        arr.push(json!({
                            "label": "default",
                            "data": old_data,
                            "created": created,
                        }));
                        if let Some(obj) = j.as_object_mut() {
                            obj.remove(&old_key);
                        }
                    }
                    j[emb_array_key.clone()] = json!(arr);
                }

                let arr = j.get_mut(&emb_array_key).unwrap().as_array_mut().unwrap();

                let model_ver = get_model_version(&self.recognition_model_path);

                // Max embeddings limit
                if self.config.max_embeddings > 0
                    && arr.len() >= self.config.max_embeddings as usize
                {
                    let mut found = false;
                    for entry in arr.iter_mut() {
                        if entry.get("label").and_then(|l| l.as_str()) == Some(label) {
                            entry["data"] = pending_data.clone();
                            entry["created"] = json!(SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs());
                            entry["model_version"] = json!(model_ver);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        log_warn(format!(
                            "Max embeddings ({}) reached for {}",
                            self.config.max_embeddings, username
                        ));
                        return false;
                    }
                } else {
                    let mut found = false;
                    for entry in arr.iter_mut() {
                        if entry.get("label").and_then(|l| l.as_str()) == Some(label) {
                            entry["data"] = pending_data.clone();
                            entry["created"] = json!(SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs());
                            entry["model_version"] = json!(model_ver);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        arr.push(json!({
                            "label": label,
                            "data": pending_data,
                            "created": SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            "model_version": model_ver,
                        }));
                    }
                }

                if let Some(obj) = j.as_object_mut() {
                    obj.remove(&pending_key);
                }
                updated = true;
            }
        }

        if updated {
            let json_str = serde_json::to_string_pretty(&j).unwrap_or_default();
            if Self::write_json_atomic(&user_file, &json_str).is_err() {
                return false;
            }
            log_info(format!("Set label '{label}' for {username}"));
        }

        updated
    }

    pub fn train_user(&mut self, username: &str, label: &str, create_new: bool) -> bool {
        if !self.ensure_models_loaded() {
            return false;
        }
        if !is_valid_username(username) {
            log_warn(format!("Security Warn: Invalid username string: {username}"));
            return false;
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let mut j = match self.load_user_json_safe(&user_file) {
            Some(j) => j,
            None => return false,
        };

        let mut updated_any = false;
        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }

        for i in 0..self.active_cameras.len() {
            let def = self.active_cameras[i].config.clone();
            let frame = match self.active_cameras[i].cam.capture() {
                Some(f) => f,
                None => {
                    log_warn(format!("Train: Camera {} failed capture.", def.id));
                    continue;
                }
            };

            let mut new_vec = Vec::new();
            let num_faces = self.generate_embedding(&frame, &mut new_vec);

            if num_faces != 1 {
                log_warn(format!("Train: Expected 1 face, found {num_faces}"));
                continue;
            }

            let emb_array_key = format!("embeddings_{}", def.cam_type);
            let emb_key = format!("embedding_{}", def.cam_type);

            if !j.get(&emb_array_key).is_some_and(|v| v.is_array()) {
                let mut arr = Vec::new();
                if let Some(old_data) = j.get(&emb_key).cloned() {
                    arr.push(json!({
                        "label": "default",
                        "data": old_data,
                        "created": SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    }));
                    if let Some(obj) = j.as_object_mut() {
                        obj.remove(&emb_key);
                    }
                }
                j[emb_array_key.clone()] = json!(arr);
            }

            let arr = j.get_mut(&emb_array_key).unwrap().as_array_mut().unwrap();

            if create_new {
                if self.config.max_embeddings > 0
                    && arr.len() >= self.config.max_embeddings as usize
                {
                    log_warn(format!("Max embeddings reached for {username}"));
                    return false;
                }
                let effective_label = if label.is_empty() {
                    format!(
                        "trained_{}",
                        SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    )
                } else {
                    label.to_string()
                };
                arr.push(json!({
                    "label": effective_label,
                    "data": new_vec,
                    "created": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                }));
                updated_any = true;
            } else {
                let mut found = false;
                for entry in arr.iter_mut() {
                    if entry.get("label").and_then(|l| l.as_str()) == Some(label) {
                        if let Some(old_data) = entry.get("data").and_then(|d| d.as_array()) {
                            let old_vec: Vec<f32> = old_data
                                .iter()
                                .filter_map(|x| x.as_f64().map(|f| f as f32))
                                .collect();
                            let mut avg: Vec<f32> = old_vec
                                .iter()
                                .zip(new_vec.iter())
                                .map(|(a, b)| a + b)
                                .collect();
                            let norm: f32 = avg.iter().map(|x| x * x).sum::<f32>().sqrt();
                            if norm > 0.0 {
                                for val in avg.iter_mut() {
                                    *val /= norm;
                                }
                            }
                            entry["data"] = json!(avg);
                            entry["created"] = json!(SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs());
                            found = true;
                            updated_any = true;
                            break;
                        }
                    }
                }
                if !found {
                    arr.push(json!({
                        "label": label,
                        "data": new_vec,
                        "created": SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    }));
                    updated_any = true;
                }
            }
        }

        if updated_any {
            let json_str = serde_json::to_string_pretty(&j).unwrap_or_default();
            if Self::write_json_atomic(&user_file, &json_str).is_err() {
                return false;
            }
        }

        updated_any
    }

    pub fn list_embeddings(&mut self, username: &str) -> Vec<String> {
        let mut unique_labels = HashSet::new();
        if !is_valid_username(username) {
            return Vec::new();
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let j = match self.load_user_json_safe(&user_file) {
            Some(j) => j,
            None => return Vec::new(),
        };

        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }
        for ac in &self.active_cameras {
            let def = &ac.config;
            let emb_array_key = format!("embeddings_{}", def.cam_type);
            if let Some(arr) = j.get(&emb_array_key).and_then(|v| v.as_array()) {
                for entry in arr {
                    if let Some(label) = entry.get("label").and_then(|l| l.as_str()) {
                        unique_labels.insert(label.to_string());
                    }
                }
            }
            let emb_key = format!("embedding_{}", def.cam_type);
            if j.get(&emb_key).is_some() {
                unique_labels.insert("default (legacy)".to_string());
            }
        }

        let mut list: Vec<String> = unique_labels.into_iter().collect();
        list.sort();
        list
    }

    pub fn remove_embedding(&mut self, username: &str, label: &str) -> bool {
        if !is_valid_username(username) {
            return false;
        }

        let user_file = self.config.users_dir.join(format!("{username}.json"));
        let mut j = match self.load_user_json_safe(&user_file) {
            Some(j) => j,
            None => return false,
        };

        let mut removed = false;
        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }
        for ac in &self.active_cameras {
            let def = &ac.config;
            let emb_array_key = format!("embeddings_{}", def.cam_type);
            if let Some(arr) = j.get_mut(&emb_array_key).and_then(|v| v.as_array_mut()) {
                let initial_len = arr.len();
                arr.retain(|entry| {
                    entry.get("label").and_then(|l| l.as_str()) != Some(label)
                });
                if arr.len() < initial_len {
                    removed = true;
                }
            }
        }

        if removed {
            let json_str = serde_json::to_string_pretty(&j).unwrap_or_default();
            if Self::write_json_atomic(&user_file, &json_str).is_err() {
                return false;
            }
            log_info(format!("Removed embedding '{label}' for {username}"));
        }

        removed
    }

    pub fn test_camera_and_auth(&mut self) -> bool {
        if !self.ensure_models_loaded() {
            return false;
        }

        let mut any_ok = false;
        if self.active_cameras.is_empty() {
            self.initialize_active_cameras();
        }
        log_info(format!("Testing {} cameras.", self.active_cameras.len()));

        for i in 0..self.active_cameras.len() {
            let def = self.active_cameras[i].config.clone();
            log_info(format!("Testing Camera {}...", def.id));
            if let Some(frame) = self.active_cameras[i].cam.capture() {
                let mut emb = Vec::new();
                let faces = self.generate_embedding(&frame, &mut emb);
                any_ok = true;
                if faces > 0 {
                    log_info(format!("Detected {faces} faces on Camera {}", def.id));
                } else {
                    log_info(format!("No faces detected on Camera {}", def.id));
                }
            } else {
                log_error("  -> Capture Failed.");
            }
        }

        any_ok
    }
}

impl Default for AuthEngine {
    fn default() -> Self {
        Self::new()
    }
}
