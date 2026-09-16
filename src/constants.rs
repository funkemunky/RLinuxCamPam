pub const LINUXCAMPAM_VERSION: &str = "0.9.7.5+ga8f02f6";

pub const SOCKET_PATH: &str = "/run/linuxcampam/socket";
pub const CONFIG_PATH: &str = "/etc/linuxcampam/config.ini";
pub const USERS_DIR: &str = "/etc/linuxcampam/users";
pub const MODELS_DIR: &str = "/usr/share/linuxcampam/models";
pub const IR_EMITTER_PATH: &str = "/usr/local/bin/linux-enable-ir-emitter";

pub const MAX_USERNAME_LENGTH: usize = 32;
pub const SECURE_FILE_MODE: u32 = 0o600;
pub const RGB_CHANNELS: f64 = 3.0;
pub const DEFAULT_MIN_UID: libc::uid_t = 1000;

// Camera & Auth constants
pub const CAMERA_WARMUP_FRAMES: usize = 10;
pub const CAMERA_WARMUP_DELAY_MS: u64 = 100;
pub const CAMERA_AVERAGE_FRAMES: usize = 5;
pub const IR_TRIGGER_DELAY_MS: u64 = 200;
pub const CAPTURE_RETRY_DELAY_S: u64 = 1;

// HDR Constants
pub const HDR_EXPOSURE_1: i32 = 50;
pub const HDR_EXPOSURE_2: i32 = 150;
pub const HDR_EXPOSURE_3: i32 = 400;
pub const HDR_SETTLE_MS: u64 = 100;
pub const HDR_BIT_DEPTH: i32 = 255;
pub const CAMERA_RGB_WEIGHT: i32 = 40;
pub const DEFAULT_MIN_BRIGHTNESS: i32 = 40;
pub const CAPTURE_RETRY_ATTEMPTS: usize = 3;

pub const MIRROR_THRESHOLD_DEFAULT: f32 = 0.6;
pub const MIRROR_SIZE: i32 = 640;
pub const MIRROR_NMS: i32 = 5000;
