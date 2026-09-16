use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use crate::constants::*;
use crate::utils::{enumerate_cameras, get_ir_emitter_version, ICameraBackend, RealCameraBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthPolicy {
    StrictAll,
    LenientAny,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximitySensorMode {
    Auto,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CameraDefinition {
    pub id: String,
    pub path: String,
    pub cam_type: String,
    pub min_brightness: i32,
    pub mandatory: bool,
    pub enroll_hdr: String,
    pub enroll_averaging: String,
    pub enroll_average_frames: i32,
}


#[derive(Debug, Clone)]
pub struct Configuration {
    pub threshold: f32,
    pub detection_threshold: f32,
    pub timeout_ms: i32,
    pub max_embeddings: i32,
    pub policy: AuthPolicy,
    pub camera_defs: Vec<CameraDefinition>,
    pub save_success: bool,
    pub save_fail: bool,
    pub log_dir: PathBuf,
    pub log_level: String,
    pub log_file: String,
    pub provider_priority: Vec<String>,
    pub model_keep_alive_sec: i32,
    pub enroll_hdr: String,
    pub enroll_averaging: bool,
    pub enroll_average_frames: i32,
    pub verify_averaging: bool,
    pub verify_average_frames: i32,
    pub proximity_sensor: ProximitySensorMode,
    pub proximity_sensor_id: String,
    pub proximity_enforce: bool,
    pub wake_enabled: bool,
    pub always_wake_on_presence_detected: bool,
    pub wake_confidence_threshold: i32,
    pub lock_enabled: bool,
    pub lock_confidence_threshold: i32,
    pub lock_timeout_seconds: i32,
    pub lock_command: String,
    pub users_dir: PathBuf,
    pub models_dir: PathBuf,
    pub ir_emitter_path: PathBuf,
    pub lockout_attempts: i32,
    pub lockout_duration_sec: i32,
    pub min_uid: libc::uid_t,
    pub gpu_flush: bool,
    pub gpu_throttle_ms: i32,
    pub detection_model: Option<PathBuf>,
    pub recognition_model: Option<PathBuf>,
}

impl Configuration {
    pub const DEFAULT_THRESHOLD: f32 = 0.363;
    pub const DEFAULT_DETECTION_THRESHOLD: f32 = 0.9;
    pub const DEFAULT_TIMEOUT_MS: i32 = 3000;
    pub const DEFAULT_MAX_EMBEDDINGS: i32 = 5;
    pub const DEFAULT_ENROLL_AVG_FRAMES: i32 = 5;
    pub const DEFAULT_VERIFY_AVG_FRAMES: i32 = 3;
    pub const DEFAULT_LOCKOUT_ATTEMPTS: i32 = 5;
    pub const DEFAULT_LOCKOUT_DURATION_SEC: i32 = 300;
    pub const DEFAULT_GPU_THROTTLE_MS: i32 = 20;
    pub const DEFAULT_WAKE_CONFIDENCE_THRESHOLD: i32 = 50;
    pub const DEFAULT_LOCK_CONFIDENCE_THRESHOLD: i32 = 5;
    pub const DEFAULT_LOCK_TIMEOUT_SECONDS: i32 = 10;

    pub fn new() -> Self {
        Self {
            threshold: Self::DEFAULT_THRESHOLD,
            detection_threshold: Self::DEFAULT_DETECTION_THRESHOLD,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
            max_embeddings: Self::DEFAULT_MAX_EMBEDDINGS,
            policy: AuthPolicy::Adaptive,
            camera_defs: Vec::new(),
            save_success: false,
            save_fail: false,
            log_dir: PathBuf::from("/var/log/linuxcampam/"),
            log_level: "info".to_string(),
            log_file: String::new(),
            provider_priority: vec!["OpenCL".to_string(), "CPU".to_string()],
            model_keep_alive_sec: 0,
            enroll_hdr: "auto".to_string(),
            enroll_averaging: true,
            enroll_average_frames: Self::DEFAULT_ENROLL_AVG_FRAMES,
            verify_averaging: false,
            verify_average_frames: Self::DEFAULT_VERIFY_AVG_FRAMES,
            proximity_sensor: ProximitySensorMode::Auto,
            proximity_sensor_id: "ITE8353".to_string(),
            proximity_enforce: false,
            wake_enabled: true,
            always_wake_on_presence_detected: true,
            wake_confidence_threshold: Self::DEFAULT_WAKE_CONFIDENCE_THRESHOLD,
            lock_enabled: false,
            lock_confidence_threshold: Self::DEFAULT_LOCK_CONFIDENCE_THRESHOLD,
            lock_timeout_seconds: Self::DEFAULT_LOCK_TIMEOUT_SECONDS,
            lock_command: "loginctl lock-sessions".to_string(),
            users_dir: PathBuf::from(USERS_DIR),
            models_dir: PathBuf::from(MODELS_DIR),
            ir_emitter_path: PathBuf::from(IR_EMITTER_PATH),
            lockout_attempts: Self::DEFAULT_LOCKOUT_ATTEMPTS,
            lockout_duration_sec: Self::DEFAULT_LOCKOUT_DURATION_SEC,
            min_uid: DEFAULT_MIN_UID,
            gpu_flush: true,
            gpu_throttle_ms: Self::DEFAULT_GPU_THROTTLE_MS,
            detection_model: None,
            recognition_model: None,
        }
    }

    pub fn load_file(
        &mut self,
        config_path: impl AsRef<Path>,
        backend: Option<&dyn ICameraBackend>,
    ) -> bool {
        let path = config_path.as_ref();
        if let Ok(file) = fs::File::open(path) {
            self.load(BufReader::new(file), backend)
        } else {
            // Loading defaults is considered valid if the file is absent
            let empty = std::io::Cursor::new(b"");
            self.load(empty, backend)
        }
    }

    pub fn load<R: Read>(
        &mut self,
        input: R,
        backend: Option<&dyn ICameraBackend>,
    ) -> bool {
        self.parse_ini_into_self(input, backend);
        true
    }

    fn parse_ini_into_self<R: Read>(
        &mut self,
        input: R,
        backend: Option<&dyn ICameraBackend>,
    ) {
        let mut ini: HashMap<String, String> = HashMap::new();
        let reader = BufReader::new(input);
        let mut current_section = String::new();

        for line_res in reader.lines() {
            let line = match line_res {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                current_section = trimmed[1..trimmed.len() - 1].trim().to_string();
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos + 1..].trim();
                let full_key = if current_section.is_empty() {
                    key.to_string()
                } else {
                    format!("{}.{}", current_section, key)
                };
                ini.insert(full_key, val.to_string());
            }
        }

        let get = |k: &str, default: &str| -> String {
            ini.get(k).cloned().unwrap_or_else(|| default.to_string())
        };

        // Threshold
        let mut th_str = get("Auth.threshold", "");
        if th_str.is_empty() {
            th_str = get("General.threshold", "");
        }
        if !th_str.is_empty() {
            if let Ok(val) = th_str.parse::<f32>() {
                self.threshold = val;
            }
        }

        // Detection threshold
        let mut dt_str = get("Auth.detection_threshold", "");
        if dt_str.is_empty() {
            dt_str = get("General.detection_threshold", "");
        }
        if !dt_str.is_empty() {
            if let Ok(val) = dt_str.parse::<f32>() {
                self.detection_threshold = val;
            }
        }

        // Timeout ms
        let mut to_str = get("Auth.timeout_ms", "");
        if to_str.is_empty() {
            to_str = get("General.timeout_ms", "");
        }
        if !to_str.is_empty() {
            if let Ok(val) = to_str.parse::<i32>() {
                self.timeout_ms = val;
            }
        }

        self.log_level = get("General.log_level", "info");
        self.log_file = get("General.log_file", "");

        // Auth policy
        let mut method = get("Auth.policy", "");
        if method.is_empty() {
            method = get("General.auth_method", "");
            if ini.contains_key("General.policy") {
                method = get("General.policy", "");
            }
        }
        match method.as_str() {
            "strict_all" | "2fa" | "strict" => self.policy = AuthPolicy::StrictAll,
            "lenient_any" | "1fa" | "lenient" => self.policy = AuthPolicy::LenientAny,
            _ => self.policy = AuthPolicy::Adaptive,
        }

        // Paths
        if ini.contains_key("Paths.users_dir") {
            self.users_dir = PathBuf::from(get("Paths.users_dir", ""));
        }
        if ini.contains_key("Paths.models_dir") {
            self.models_dir = PathBuf::from(get("Paths.models_dir", ""));
        }
        if ini.contains_key("Paths.ir_emitter_path") {
            self.ir_emitter_path = PathBuf::from(get("Paths.ir_emitter_path", ""));
        }
        if ini.contains_key("Paths.detection_model") {
            self.detection_model = Some(PathBuf::from(get("Paths.detection_model", "")));
        }
        if ini.contains_key("Paths.recognition_model") {
            self.recognition_model = Some(PathBuf::from(get("Paths.recognition_model", "")));
        }

        // Max embeddings
        let mut me_str = get("Auth.max_embeddings", "");
        if me_str.is_empty() {
            me_str = get("General.max_embeddings", "");
        }
        if !me_str.is_empty() {
            if let Ok(val) = me_str.parse::<i32>() {
                self.max_embeddings = val;
            }
        }

        // Capture settings
        if ini.contains_key("Capture.enroll_hdr") {
            self.enroll_hdr = get("Capture.enroll_hdr", "");
        }
        if ini.contains_key("Capture.enroll_averaging") {
            let val = get("Capture.enroll_averaging", "");
            self.enroll_averaging = val == "on" || val == "true";
        }
        if let Ok(val) = get(
            "Capture.enroll_average_frames",
            &Self::DEFAULT_ENROLL_AVG_FRAMES.to_string(),
        )
        .parse::<i32>()
        {
            self.enroll_average_frames = val;
        }
        if ini.contains_key("Capture.verify_averaging") {
            let val = get("Capture.verify_averaging", "");
            self.verify_averaging = val == "on" || val == "true";
        }
        if let Ok(val) = get(
            "Capture.verify_average_frames",
            &Self::DEFAULT_VERIFY_AVG_FRAMES.to_string(),
        )
        .parse::<i32>()
        {
            self.verify_average_frames = val;
        }

        // Cameras
        self.camera_defs.clear();
        let cam_names = get("Cameras.names", "");
        if !cam_names.is_empty() {
            for id_raw in cam_names.split(',') {
                let id = id_raw.trim();
                if id.is_empty() {
                    continue;
                }
                let mut def = CameraDefinition {
                    id: id.to_string(),
                    path: get(&format!("Camera.{}.path", id), "/dev/video0"),
                    cam_type: get(&format!("Camera.{}.type", id), "generic"),
                    min_brightness: 0,
                    mandatory: get(&format!("Camera.{}.mandatory", id), "false") == "true",
                    enroll_hdr: get(&format!("Camera.{}.enroll_hdr", id), ""),
                    enroll_averaging: get(&format!("Camera.{}.enroll_averaging", id), ""),
                    enroll_average_frames: 0,
                };
                if let Ok(val) =
                    get(&format!("Camera.{}.min_brightness", id), "0").parse::<i32>()
                {
                    def.min_brightness = val;
                }
                if let Ok(val) =
                    get(&format!("Camera.{}.enroll_average_frames", id), "0").parse::<i32>()
                {
                    def.enroll_average_frames = val;
                }
                self.camera_defs.push(def);
            }
        } else {
            let path_ir = get("Hardware.camera_path_ir", "");
            let path_rgb = get("Hardware.camera_path_rgb", "");
            if !path_ir.is_empty() || !path_rgb.is_empty() {
                if !path_ir.is_empty() {
                    self.camera_defs.push(CameraDefinition {
                        id: "ir".to_string(),
                        path: path_ir,
                        cam_type: "ir".to_string(),
                        min_brightness: 0,
                        mandatory: true,
                        ..Default::default()
                    });
                }
                if !path_rgb.is_empty() {
                    let mut mb = DEFAULT_MIN_BRIGHTNESS;
                    if let Ok(val) = get(
                        "Hardware.min_brightness",
                        &DEFAULT_MIN_BRIGHTNESS.to_string(),
                    )
                    .parse::<i32>()
                    {
                        mb = val;
                    }
                    self.camera_defs.push(CameraDefinition {
                        id: "rgb".to_string(),
                        path: path_rgb,
                        cam_type: "rgb".to_string(),
                        min_brightness: mb,
                        mandatory: false,
                        ..Default::default()
                    });
                }
            } else {
                // Auto detect
                let real_backend = RealCameraBackend::new();
                let b: &dyn ICameraBackend = backend.unwrap_or(&real_backend);
                let detected = enumerate_cameras(b);

                if !detected.empty_check() {
                    let mut ir_p = String::new();
                    let mut rgb_p = String::new();
                    for (dev_path, cam_type) in &detected {
                        if cam_type == "ir" && ir_p.is_empty() {
                            ir_p = dev_path.clone();
                        } else if (cam_type == "rgb" || cam_type == "generic") && rgb_p.is_empty() {
                            rgb_p = dev_path.clone();
                        }
                    }

                    if !ir_p.is_empty() && !rgb_p.is_empty() {
                        self.camera_defs.push(CameraDefinition {
                            id: "ir".to_string(),
                            path: ir_p,
                            cam_type: "ir".to_string(),
                            min_brightness: 0,
                            mandatory: true,
                            ..Default::default()
                        });
                        self.camera_defs.push(CameraDefinition {
                            id: "rgb".to_string(),
                            path: rgb_p,
                            cam_type: "rgb".to_string(),
                            min_brightness: CAMERA_RGB_WEIGHT,
                            mandatory: false,
                            ..Default::default()
                        });
                    } else if !rgb_p.is_empty() {
                        self.camera_defs.push(CameraDefinition {
                            id: "rgb".to_string(),
                            path: rgb_p,
                            cam_type: "rgb".to_string(),
                            min_brightness: 0,
                            mandatory: true,
                            ..Default::default()
                        });
                    } else if !ir_p.is_empty() {
                        self.camera_defs.push(CameraDefinition {
                            id: "ir".to_string(),
                            path: ir_p,
                            cam_type: "ir".to_string(),
                            min_brightness: 0,
                            mandatory: true,
                            ..Default::default()
                        });
                    } else {
                        self.camera_defs.push(CameraDefinition {
                            id: "cam0".to_string(),
                            path: detected[0].0.clone(),
                            cam_type: "generic".to_string(),
                            min_brightness: 0,
                            mandatory: true,
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // Hardware / Provider
        self.provider_priority.clear();
        let priority_str = get("Hardware.provider_priority", "");
        if !priority_str.is_empty() {
            for segment in priority_str.split(',') {
                let trimmed = segment.trim();
                if !trimmed.is_empty() {
                    self.provider_priority.push(trimmed.to_string());
                }
            }
        }
        if self.provider_priority.is_empty() {
            self.provider_priority = vec!["OpenCL".to_string(), "CPU".to_string()];
        }

        let prox_str = get("Hardware.proximity_sensor", "auto");
        self.proximity_sensor = match prox_str.as_str() {
            "true" | "on" | "enabled" => ProximitySensorMode::Enabled,
            "false" | "off" | "disabled" => ProximitySensorMode::Disabled,
            _ => ProximitySensorMode::Auto,
        };
        self.proximity_sensor_id = get("Hardware.proximity_sensor_id", "ITE8353");
        self.proximity_enforce = get("Hardware.proximity_enforce", "false") == "true";

        // Proximity Wake / Lock
        self.wake_enabled = get("Proximity.wake_enabled", "true") == "true";
        self.always_wake_on_presence_detected =
            get("Proximity.always_wake_on_presence_detected", "true") == "true";
        if let Ok(val) = get("Proximity.wake_confidence_threshold", "50").parse::<i32>() {
            self.wake_confidence_threshold = val;
        }
        self.lock_enabled = get("Proximity.lock_enabled", "false") == "true";
        if let Ok(val) = get("Proximity.lock_confidence_threshold", "5").parse::<i32>() {
            self.lock_confidence_threshold = val;
        }
        if let Ok(val) = get("Proximity.lock_timeout_seconds", "10").parse::<i32>() {
            self.lock_timeout_seconds = val;
        }
        self.lock_command = get("Proximity.lock_command", "loginctl lock-sessions");

        // Storage & Performance
        self.save_success = get("Storage.save_success_images", "false") == "true";
        self.save_fail = get("Storage.save_fail_images", "false") == "true";
        if let Ok(val) = get("Performance.model_keep_alive_sec", "0").parse::<i32>() {
            self.model_keep_alive_sec = val;
        }
        if let Ok(val) = get("Security.lockout_attempts", "5").parse::<i32>() {
            self.lockout_attempts = val;
        }
        if let Ok(val) = get("Security.lockout_duration_sec", "300").parse::<i32>() {
            self.lockout_duration_sec = val;
        }

        // min_uid
        let mut uid_str = get("Security.min_uid", "");
        if uid_str.is_empty() {
            uid_str = get("General.min_uid", "");
        }
        let mut min_uid_int = DEFAULT_MIN_UID as i32;
        if !uid_str.is_empty() {
            if let Ok(val) = uid_str.parse::<i32>() {
                min_uid_int = val;
            }
        }
        if min_uid_int < 0 {
            min_uid_int = DEFAULT_MIN_UID as i32;
        }
        self.min_uid = min_uid_int as libc::uid_t;

        self.gpu_flush = get("Performance.gpu_flush", "on") == "on";
        if let Ok(val) = get("Performance.gpu_throttle_ms", "20").parse::<i32>() {
            self.gpu_throttle_ms = val;
        }
    }

    pub fn to_string_formatted(&self) -> String {
        let mut out = String::from("=== Active Configuration ===\n\n");
        out.push_str("[General]\n");
        out.push_str(&format!("  Log Level: {}\n", self.log_level));
        if !self.log_file.is_empty() {
            out.push_str(&format!("  Log File: {}\n", self.log_file));
        }
        out.push_str(&format!("  Threshold: {}\n", self.threshold));
        out.push_str(&format!(
            "  Detection Threshold: {}\n",
            self.detection_threshold
        ));
        out.push_str(&format!("  Timeout: {} ms\n", self.timeout_ms));

        let prox_mode_str = match self.proximity_sensor {
            ProximitySensorMode::Enabled => "enabled",
            ProximitySensorMode::Disabled => "disabled",
            ProximitySensorMode::Auto => "auto",
        };
        out.push_str(&format!("  Proximity Sensor: {}\n", prox_mode_str));
        out.push_str(&format!(
            "  Proximity Sensor ID: {}\n",
            self.proximity_sensor_id
        ));
        out.push_str(&format!(
            "  Proximity Enforce: {}\n",
            if self.proximity_enforce {
                "true"
            } else {
                "false"
            }
        ));
        out.push_str(&format!(
            "  Wake Enabled: {} (Always on return: {}, Threshold: {}%)\n",
            if self.wake_enabled { "true" } else { "false" },
            if self.always_wake_on_presence_detected {
                "true"
            } else {
                "false"
            },
            self.wake_confidence_threshold
        ));
        out.push_str(&format!(
            "  Lock Enabled: {} (Threshold: {}%, Timeout: {}s)\n",
            if self.lock_enabled { "true" } else { "false" },
            self.lock_confidence_threshold,
            self.lock_timeout_seconds
        ));
        out.push_str(&format!("  Lock Command: {}\n", self.lock_command));

        out.push_str("  Auth Policy: ");
        match self.policy {
            AuthPolicy::Adaptive => {
                out.push_str("Adaptive (IR Strict, RGB Conditional)\n");
            }
            AuthPolicy::StrictAll => {
                out.push_str("Strict (All Cameras Must Match)\n");
            }
            AuthPolicy::LenientAny => {
                out.push_str("Lenient (Any Camera Match)\n");
            }
        }
        out.push_str(&format!("  Max Embeddings: {}\n\n", self.max_embeddings));

        out.push_str("[Security]\n");
        out.push_str(&format!("  Lockout Attempts: {}\n", self.lockout_attempts));
        out.push_str(&format!(
            "  Lockout Duration: {} s\n\n",
            self.lockout_duration_sec
        ));

        out.push_str(&format!(
            "[Cameras] ({} active)\n",
            self.camera_defs.len()
        ));
        for cam in &self.camera_defs {
            out.push_str(&format!("  - ID: {}\n", cam.id));
            out.push_str(&format!("    Path: {}\n", cam.path));
            out.push_str(&format!("    Type: {}\n", cam.cam_type));
            out.push_str(&format!(
                "    Mandatory: {}\n",
                if cam.mandatory { "Yes" } else { "No" }
            ));
            out.push_str(&format!("    Min Brightness: {}\n", cam.min_brightness));
            if !cam.enroll_hdr.is_empty() {
                out.push_str(&format!("    Enroll HDR: {}\n", cam.enroll_hdr));
            }
            if cam.cam_type == "ir" {
                let ir_ver = get_ir_emitter_version(&self.ir_emitter_path.to_string_lossy());
                if !ir_ver.is_empty() {
                    out.push_str(&format!(
                        "    IR Emitter Path: {}\n",
                        self.ir_emitter_path.to_string_lossy()
                    ));
                    out.push_str(&format!("    IR Emitter Version: {}\n", ir_ver));
                } else {
                    out.push_str("    IR Emitter: Not Installed\n");
                }
            }
            out.push('\n');
        }

        out.push_str("[Performance]\n");
        out.push_str(&format!(
            "  GPU Flush: {}\n",
            if self.gpu_flush { "On" } else { "Off" }
        ));
        out.push_str(&format!("  GPU Throttle: {} ms\n", self.gpu_throttle_ms));
        out.push_str("  Provider Priority: ");
        for (i, p) in self.provider_priority.iter().enumerate() {
            out.push_str(p);
            if i < self.provider_priority.len() - 1 {
                out.push_str(" > ");
            }
        }
        out.push('\n');

        out
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self::new()
    }
}

trait EmptyCheck {
    fn empty_check(&self) -> bool;
}

impl<T> EmptyCheck for Vec<T> {
    fn empty_check(&self) -> bool {
        self.is_empty()
    }
}
