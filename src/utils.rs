use std::ffi::{CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

use crate::constants::MAX_USERNAME_LENGTH;

pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x00000001;
pub const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;

pub const V4L2_PIX_FMT_GREY: u32 = 0x59455247;
pub const V4L2_PIX_FMT_Y10: u32 = 0x20303159;
pub const V4L2_PIX_FMT_Y12: u32 = 0x20323159;
pub const V4L2_PIX_FMT_Y16: u32 = 0x20363159;
pub const V4L2_PIX_FMT_MJPEG: u32 = 0x47504a4d;
pub const V4L2_PIX_FMT_YUYV: u32 = 0x56595559;
pub const V4L2_PIX_FMT_UYVY: u32 = 0x59565955;
pub const V4L2_PIX_FMT_NV12: u32 = 0x3231564e;
pub const V4L2_PIX_FMT_RGB24: u32 = 0x33424752;
pub const V4L2_PIX_FMT_BGR24: u32 = 0x33524742;

pub const V4L2_MEMORY_MMAP: u32 = 1;
pub const VIDIOC_QUERYCAP: libc::c_ulong = 0x80685600;
pub const VIDIOC_ENUM_FMT: libc::c_ulong = 0xc0405602;
pub const VIDIOC_G_FMT: libc::c_ulong = 0xc0d05604;
pub const VIDIOC_REQBUFS: libc::c_ulong = 0xc0145608;
pub const VIDIOC_QUERYBUF: libc::c_ulong = 0xc0585609;
pub const VIDIOC_QBUF: libc::c_ulong = 0xc058560f;
pub const VIDIOC_DQBUF: libc::c_ulong = 0xc0585611;
pub const VIDIOC_STREAMON: libc::c_ulong = 0x40045612;
pub const VIDIOC_STREAMOFF: libc::c_ulong = 0x40045613;
pub const VIDIOC_G_CTRL: libc::c_ulong = 0xc008561b;
pub const VIDIOC_S_CTRL: libc::c_ulong = 0xc008561c;
pub const VIDIOC_QUERYCTRL: libc::c_ulong = 0xc0445624;
pub const V4L2_CID_EXPOSURE_AUTO: u32 = 0x009a0901;
pub const V4L2_CID_EXPOSURE_ABSOLUTE: u32 = 0x9a0902;
pub const V4L2_CTRL_FLAG_DISABLED: u32 = 0x0001;

#[repr(C)]
struct V4l2Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct V4l2Fmtdesc {
    index: u32,
    type_: u32,
    flags: u32,
    description: [u8; 32],
    pixelformat: u32,
    mbus_code: u32,
    reserved: [u32; 3],
}

#[repr(C)]
pub struct V4l2Queryctrl {
    pub id: u32,
    pub type_: u32,
    pub name: [u8; 32],
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub default_value: i32,
    pub flags: u32,
    pub reserved: [u32; 2],
}

pub trait ICameraBackend: Send + Sync {
    fn get_device_paths(&self) -> Vec<String>;
    fn is_video_capture_device(&self, path: &str) -> bool;
    fn get_pixel_formats(&self, path: &str) -> Vec<u32>;
}

pub struct RealCameraBackend;

impl RealCameraBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RealCameraBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ICameraBackend for RealCameraBackend {
    fn get_device_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if let Ok(entries) = fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("video") {
                    paths.push(entry.path().to_string_lossy().into_owned());
                }
            }
        }
        paths
    }

    fn is_video_capture_device(&self, path: &str) -> bool {
        let c_path = match CString::new(path) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 {
            return false;
        }

        let mut cap: V4l2Capability = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::ioctl(fd, VIDIOC_QUERYCAP, &mut cap) };
        unsafe { libc::close(fd) };

        if ret < 0 {
            return false;
        }
        let caps = if (cap.capabilities & 0x80000000) != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };
        (caps & V4L2_CAP_VIDEO_CAPTURE) != 0
    }

    fn get_pixel_formats(&self, path: &str) -> Vec<u32> {
        let mut formats = Vec::new();
        let c_path = match CString::new(path) {
            Ok(p) => p,
            Err(_) => return formats,
        };
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 {
            return formats;
        }

        let mut index = 0u32;
        loop {
            let mut fmt: V4l2Fmtdesc = unsafe { std::mem::zeroed() };
            fmt.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            fmt.index = index;
            let ret = unsafe { libc::ioctl(fd, VIDIOC_ENUM_FMT, &mut fmt) };
            if ret != 0 {
                break;
            }
            formats.push(fmt.pixelformat);
            index += 1;
        }
        unsafe { libc::close(fd) };
        formats
    }
}

pub fn classify_camera_type(device_path: &str, backend: &dyn ICameraBackend) -> String {
    if !backend.is_video_capture_device(device_path) {
        return String::new();
    }

    let formats = backend.get_pixel_formats(device_path);
    let mut has_grey = false;
    let mut has_color = false;

    for &fmt in &formats {
        if fmt == V4L2_PIX_FMT_GREY
            || fmt == V4L2_PIX_FMT_Y10
            || fmt == V4L2_PIX_FMT_Y12
            || fmt == V4L2_PIX_FMT_Y16
        {
            has_grey = true;
        }
        if fmt == V4L2_PIX_FMT_MJPEG
            || fmt == V4L2_PIX_FMT_YUYV
            || fmt == V4L2_PIX_FMT_UYVY
            || fmt == V4L2_PIX_FMT_NV12
            || fmt == V4L2_PIX_FMT_RGB24
            || fmt == V4L2_PIX_FMT_BGR24
        {
            has_color = true;
        }
    }

    if has_grey && !has_color {
        "ir".to_string()
    } else if has_color {
        "rgb".to_string()
    } else if has_grey {
        "ir".to_string()
    } else {
        "generic".to_string()
    }
}

pub fn enumerate_cameras(backend: &dyn ICameraBackend) -> Vec<(String, String)> {
    let mut cameras = Vec::new();
    let paths = backend.get_device_paths();

    for path in paths {
        let cam_type = classify_camera_type(&path, backend);
        if !cam_type.is_empty() {
            cameras.push((path, cam_type));
        }
    }
    cameras.sort_by(|a, b| a.0.cmp(&b.0));
    cameras
}

pub fn poll_remaining_ms(deadline: Instant, now: Instant) -> i32 {
    if now >= deadline {
        0
    } else {
        let duration = deadline - now;
        let millis = duration.as_millis();
        if millis > i32::MAX as u128 {
            i32::MAX
        } else {
            millis as i32
        }
    }
}

pub fn get_ir_emitter_version(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let is_safe = |c: char| c.is_ascii_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '-';
    if !path.chars().all(is_safe) {
        return String::new();
    }
    if !Path::new(path).exists() {
        return String::new();
    }

    match ProcessCommand::new(path).arg("-V").output() {
        Ok(output) if output.status.success() => {
            let mut version = String::from_utf8_lossy(&output.stdout).into_owned();
            while version.ends_with('\n') || version.ends_with('\r') {
                version.pop();
            }
            version
        }
        _ => String::new(),
    }
}

pub fn execute_command_spawn(command_line: &str) -> i32 {
    const MAX_COMMAND_LENGTH: usize = 4096;
    if command_line.is_empty() || command_line.len() > MAX_COMMAND_LENGTH {
        return -1;
    }

    // Reject unprintable control characters (except whitespace)
    for c in command_line.chars() {
        if c.is_control() && !c.is_whitespace() {
            return -1;
        }
    }

    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for c in command_line.chars() {
        if escape_next {
            current_arg.push(c);
            escape_next = false;
            continue;
        }
        if c == '\\' {
            escape_next = true;
            continue;
        }
        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }
        if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }
        if c.is_whitespace() && !in_single_quote && !in_double_quote {
            if !current_arg.is_empty() {
                args.push(current_arg.clone());
                current_arg.clear();
            }
            continue;
        }
        current_arg.push(c);
    }
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    if args.is_empty() {
        return -1;
    }

    match ProcessCommand::new(&args[0]).args(&args[1..]).status() {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => e.raw_os_error().unwrap_or(-1),
    }
}

pub fn get_model_version(model_path: impl AsRef<Path>) -> String {
    let p = model_path.as_ref();
    let filename = p.file_stem().unwrap_or_default().to_string_lossy();
    let prefix = "face_recognition_";
    if let Some(stripped) = filename.strip_prefix(prefix) {
        stripped.to_string()
    } else {
        filename.into_owned()
    }
}

pub fn is_valid_username(username: &str) -> bool {
    if username.is_empty() || username.len() > MAX_USERNAME_LENGTH {
        return false;
    }

    if username.starts_with('.') {
        return false;
    }

    if username.contains("..") {
        return false;
    }

    username.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' || c == '$'
    })
}

pub fn get_home_dir(uid: libc::uid_t) -> Option<PathBuf> {
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    const GETPWUID_BUFFER_SIZE: usize = 16384;
    let mut buf = vec![0 as libc::c_char; GETPWUID_BUFFER_SIZE];

    let ret = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if ret == 0 && !result.is_null() {
        let dir_cstr = unsafe { CStr::from_ptr(pwd.pw_dir) };
        Some(PathBuf::from(dir_cstr.to_string_lossy().into_owned()))
    } else {
        None
    }
}

pub fn get_uid_for_username(username: &str) -> Option<libc::uid_t> {
    let c_username = CString::new(username).ok()?;
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    const GETPWNAM_BUFFER_SIZE: usize = 16384;
    let mut buf = vec![0 as libc::c_char; GETPWNAM_BUFFER_SIZE];

    let ret = unsafe {
        libc::getpwnam_r(
            c_username.as_ptr(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if ret == 0 && !result.is_null() {
        Some(pwd.pw_uid)
    } else {
        None
    }
}
