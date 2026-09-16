pub mod auth_engine;
pub mod camera;
pub mod config;
pub mod constants;
pub mod hardware_manager;
pub mod ipc_protocol;
pub mod logger;
pub mod pam;
pub mod parsers;
pub mod presence_tripwire;
pub mod scoped_worker;
pub mod sensor_factory;
pub mod sensor_parser;
pub mod utils;
pub mod virtual_keyboard;

pub use auth_engine::{cosine_similarity, AuthEngine, AuthResult, CameraFactory};
pub use camera::{Camera, ICamera};
pub use config::{AuthPolicy, CameraDefinition, Configuration, ProximitySensorMode};
pub use constants::*;
pub use hardware_manager::{HardwareId, HardwareManager};
pub use ipc_protocol::{command_to_string, string_to_command, Command, Request};
pub use logger::{log_debug, log_error, log_info, log_warn, LogLevel, Logger};
pub use parsers::Ite8353Parser;
pub use presence_tripwire::{PresenceCallback, PresenceTripwire, PresenceTripwireHidOps};
pub use scoped_worker::ScopedWorker;
pub use sensor_factory::{ISensorFactory, SensorFactory};
pub use sensor_parser::{SensorParser, SensorState};
pub use utils::{
    classify_camera_type, enumerate_cameras, execute_command_spawn, get_home_dir,
    get_ir_emitter_version, get_model_version, get_uid_for_username, is_valid_username,
    poll_remaining_ms, ICameraBackend, RealCameraBackend,
};
pub use virtual_keyboard::VirtualKeyboard;

// --- PAM C ABI Exported Functions ---

/// PAM setcred handler.
///
/// # Safety
///
/// Must be called with valid PAM arguments according to the PAM SPI specification.
#[no_mangle]
pub unsafe extern "C" fn pam_sm_setcred(
    _pamh: *mut pam::PamHandle,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *mut *const libc::c_char,
) -> libc::c_int {
    pam::PAM_SUCCESS
}

/// PAM account management handler.
///
/// # Safety
///
/// Must be called with valid PAM arguments according to the PAM SPI specification.
#[no_mangle]
pub unsafe extern "C" fn pam_sm_acct_mgmt(
    _pamh: *mut pam::PamHandle,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *mut *const libc::c_char,
) -> libc::c_int {
    pam::PAM_SUCCESS
}

/// PAM authentication handler.
///
/// # Safety
///
/// Must be called with valid PAM arguments according to the PAM SPI specification.
#[no_mangle]
pub unsafe extern "C" fn pam_sm_authenticate(
    pamh: *mut pam::PamHandle,
    flags: libc::c_int,
    argc: libc::c_int,
    argv: *mut *const libc::c_char,
) -> libc::c_int {
    pam::authenticate_impl(pamh, flags, argc, argv)
}
