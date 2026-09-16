use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::constants::{CONFIG_PATH, LINUXCAMPAM_VERSION, SOCKET_PATH};
use crate::ipc_protocol::{Command, Request};
use crate::pam::pam_config::load_pam_config;

pub const PAM_SUCCESS: libc::c_int = 0;
pub const PAM_AUTH_ERR: libc::c_int = 7;
pub const PAM_AUTHINFO_UNAVAIL: libc::c_int = 9;
pub const PAM_USER_UNKNOWN: libc::c_int = 10;
pub const PAM_IGNORE: libc::c_int = 25;

pub const PAM_SERVICE: libc::c_int = 1;
pub const PAM_CONV: libc::c_int = 5;
pub const PAM_AUTHTOK: libc::c_int = 6;

pub const PAM_SILENT: libc::c_int = 0x8000;
pub const PAM_PROMPT_ECHO_OFF: libc::c_int = 1;
pub const PAM_TEXT_INFO: libc::c_int = 4;

#[repr(C)]
pub struct PamHandle {
    _unused: [u8; 0],
}

#[repr(C)]
pub struct PamMessage {
    pub msg_style: libc::c_int,
    pub msg: *const libc::c_char,
}

#[repr(C)]
pub struct PamResponse {
    pub resp: *mut libc::c_char,
    pub resp_retcode: libc::c_int,
}

#[repr(C)]
pub struct PamConv {
    pub conv: Option<
        unsafe extern "C" fn(
            num_msg: libc::c_int,
            msg: *mut *const PamMessage,
            resp: *mut *mut PamResponse,
            appdata_ptr: *mut libc::c_void,
        ) -> libc::c_int,
    >,
    pub appdata_ptr: *mut libc::c_void,
}

extern "C" {
    fn pam_get_user(
        pamh: *mut PamHandle,
        user: *mut *const libc::c_char,
        prompt: *const libc::c_char,
    ) -> libc::c_int;

    fn pam_get_item(
        pamh: *const PamHandle,
        item_type: libc::c_int,
        item: *mut *const libc::c_void,
    ) -> libc::c_int;

    fn pam_set_item(
        pamh: *mut PamHandle,
        item_type: libc::c_int,
        item: *const libc::c_void,
    ) -> libc::c_int;
}

static OPENLOG_ONCE: std::sync::Once = std::sync::Once::new();

unsafe fn syslog_msg(priority: libc::c_int, msg: &str) {
    OPENLOG_ONCE.call_once(|| {
        libc::openlog(
            c"LinuxCamPAM".as_ptr(),
            libc::LOG_PID | libc::LOG_NDELAY,
            libc::LOG_AUTHPRIV,
        );
    });
    if let Ok(c_msg) = CString::new(msg) {
        let fmt = b"%s\0";
        libc::syslog(priority, fmt.as_ptr() as *const libc::c_char, c_msg.as_ptr());
    }
}

/// Authenticates the user via PAM.
///
/// # Safety
///
/// `pamh` must be a valid pointer to a `PamHandle`, and `argv` must either be null
/// or point to an array of valid null-terminated C string pointers of length `argc`.
pub unsafe fn authenticate_impl(
    pamh: *mut PamHandle,
    flags: libc::c_int,
    argc: libc::c_int,
    argv: *mut *const libc::c_char,
) -> libc::c_int {
    let mut user_ptr: *const libc::c_char = std::ptr::null();
    let retval = pam_get_user(pamh, &mut user_ptr, std::ptr::null());
    if retval != PAM_SUCCESS || user_ptr.is_null() {
        return retval;
    }

    let username = CStr::from_ptr(user_ptr).to_string_lossy().into_owned();
    let mut config = load_pam_config(CONFIG_PATH);

    // Check argv for no_welcome
    if !argv.is_null() && argc > 0 {
        for i in 0..argc {
            let arg_ptr = *argv.offset(i as isize);
            if !arg_ptr.is_null() {
                if let Ok(arg) = CStr::from_ptr(arg_ptr).to_str() {
                    if arg == "no_welcome" {
                        config.show_welcome = false;
                    }
                }
            }
        }
    }

    // Check user in system passwd database
    let mut pwd: libc::passwd = std::mem::zeroed();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    const DEFAULT_PW_BUF_SIZE: usize = 16384;
    let mut buffer = vec![0 as libc::c_char; DEFAULT_PW_BUF_SIZE];
    let c_user = CString::new(username.as_str()).unwrap_or_default();

    let pw_ret = libc::getpwnam_r(
        c_user.as_ptr(),
        &mut pwd,
        buffer.as_mut_ptr(),
        buffer.len(),
        &mut result,
    );

    if pw_ret == 0 && !result.is_null() {
        if config.min_uid > 0 && pwd.pw_uid < config.min_uid {
            syslog_msg(
                libc::LOG_INFO,
                &format!(
                    "Skipping auth for system user: {} (UID {} < {})",
                    username, pwd.pw_uid, config.min_uid
                ),
            );
            return PAM_IGNORE;
        }
    } else {
        syslog_msg(
            libc::LOG_WARNING,
            &format!("User not found in system database: {username}"),
        );
        return PAM_USER_UNKNOWN;
    }

    if (flags & PAM_SILENT) != 0 {
        syslog_msg(
            libc::LOG_DEBUG,
            &format!("pam_linuxcampam version {LINUXCAMPAM_VERSION}"),
        );
    } else {
        syslog_msg(
            libc::LOG_INFO,
            &format!("pam_linuxcampam version {LINUXCAMPAM_VERSION}"),
        );
    }

    // Confirmation logic
    if config.require_confirmation {
        let mut service_ptr: *const libc::c_void = std::ptr::null();
        let s_ret = pam_get_item(pamh, PAM_SERVICE, &mut service_ptr);
        let service_name = if s_ret == PAM_SUCCESS && !service_ptr.is_null() {
            CStr::from_ptr(service_ptr as *const libc::c_char)
                .to_string_lossy()
                .into_owned()
        } else {
            syslog_msg(
                libc::LOG_WARNING,
                "PAM_SERVICE unavailable; applying confirmation prompt as fallback",
            );
            "unknown".to_string()
        };

        if service_ptr.is_null()
            || !config
                .confirmation_exempt_services
                .contains(&service_name)
        {
            let prompt_text = CString::new(
                "Press <Enter> to authenticate with face, or type password:",
            )
            .unwrap();
            let msg = PamMessage {
                msg_style: PAM_PROMPT_ECHO_OFF,
                msg: prompt_text.as_ptr(),
            };
            let mut msg_arr = [&msg as *const PamMessage];
            let mut resp_ptr: *mut PamResponse = std::ptr::null_mut();

            let mut conv_ptr: *const libc::c_void = std::ptr::null();
            if pam_get_item(pamh, PAM_CONV, &mut conv_ptr) == PAM_SUCCESS
                && !conv_ptr.is_null()
            {
                let conv = &*(conv_ptr as *const PamConv);
                if let Some(conv_fn) = conv.conv {
                    let ret = conv_fn(
                        1,
                        msg_arr.as_mut_ptr(),
                        &mut resp_ptr,
                        conv.appdata_ptr,
                    );
                    if ret != PAM_SUCCESS {
                        syslog_msg(
                            libc::LOG_ERR,
                            &format!(
                                "Authentication confirmation failed or canceled for service: {service_name}"
                            ),
                        );
                        return PAM_AUTH_ERR;
                    }

                    if !resp_ptr.is_null() {
                        let resp_str = if !(*resp_ptr).resp.is_null() {
                            CStr::from_ptr((*resp_ptr).resp).to_string_lossy().into_owned()
                        } else {
                            String::new()
                        };

                        if !resp_str.is_empty() {
                            syslog_msg(
                                libc::LOG_INFO,
                                "User provided password input, skipping face auth to allow fallback.",
                            );
                            if let Ok(c_tok) = CString::new(resp_str) {
                                pam_set_item(
                                    pamh,
                                    PAM_AUTHTOK,
                                    c_tok.as_ptr() as *const libc::c_void,
                                );
                            }
                            // Clean up response
                            if !(*resp_ptr).resp.is_null() {
                                libc::free((*resp_ptr).resp as *mut libc::c_void);
                            }
                            libc::free(resp_ptr as *mut libc::c_void);
                            return PAM_IGNORE;
                        }

                        if !(*resp_ptr).resp.is_null() {
                            libc::free((*resp_ptr).resp as *mut libc::c_void);
                        }
                        libc::free(resp_ptr as *mut libc::c_void);
                    }
                }
            } else {
                syslog_msg(libc::LOG_ERR, "Failed to get PAM_CONV for confirmation");
                return PAM_AUTHINFO_UNAVAIL;
            }
        }
    }

    // Connect to socket
    let mut stream = match UnixStream::connect(SOCKET_PATH) {
        Ok(s) => s,
        Err(_) => {
            syslog_msg(
                libc::LOG_INFO,
                "Could not connect to linuxcampamd socket - service may not be running",
            );
            return PAM_AUTHINFO_UNAVAIL;
        }
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let req = Request::new(Command::AuthRequest, vec![username.clone()]);
    let req_str = req.serialize();

    if stream.write_all(req_str.as_bytes()).is_err() {
        syslog_msg(libc::LOG_ERR, "Failed to send auth request to socket");
        return PAM_AUTHINFO_UNAVAIL;
    }

    let mut buf = [0u8; 128];
    let n = match stream.read(&mut buf) {
        Ok(bytes) if bytes > 0 => bytes,
        _ => {
            syslog_msg(libc::LOG_ERR, "Failed to read response from service");
            return PAM_AUTH_ERR;
        }
    };

    let response = String::from_utf8_lossy(&buf[..n]);
    if response.contains("AUTH_SUCCESS") {
        if config.show_welcome && ((flags & PAM_SILENT) == 0) {
            let welcome_msg = config.welcome_message.replace("%u", &username);
            if !welcome_msg.is_empty() {
                if let Ok(c_msg) = CString::new(welcome_msg) {
                    let msg = PamMessage {
                        msg_style: PAM_TEXT_INFO,
                        msg: c_msg.as_ptr(),
                    };
                    let mut msg_arr = [&msg as *const PamMessage];
                    let mut resp_ptr: *mut PamResponse = std::ptr::null_mut();

                    let mut conv_ptr: *const libc::c_void = std::ptr::null();
                    if pam_get_item(pamh, PAM_CONV, &mut conv_ptr) == PAM_SUCCESS
                        && !conv_ptr.is_null()
                    {
                        let conv = &*(conv_ptr as *const PamConv);
                        if let Some(conv_fn) = conv.conv {
                            conv_fn(1, msg_arr.as_mut_ptr(), &mut resp_ptr, conv.appdata_ptr);
                            if !resp_ptr.is_null() {
                                if !(*resp_ptr).resp.is_null() {
                                    libc::free((*resp_ptr).resp as *mut libc::c_void);
                                }
                                libc::free(resp_ptr as *mut libc::c_void);
                            }
                        }
                    }
                }
            }
        }
        syslog_msg(
            libc::LOG_INFO,
            &format!("Authentication successful for user: {username}"),
        );
        PAM_SUCCESS
    } else {
        syslog_msg(
            libc::LOG_NOTICE,
            &format!(
                "Authentication failed for user: {} (Response: {})",
                username, response
            ),
        );
        PAM_AUTH_ERR
    }
}
