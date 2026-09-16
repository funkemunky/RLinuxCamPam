use std::collections::HashMap;
use std::io::Cursor;

use pam_linuxcampam::config::Configuration;
use pam_linuxcampam::utils::{ICameraBackend, V4L2_PIX_FMT_GREY, V4L2_PIX_FMT_MJPEG, V4L2_PIX_FMT_RGB24};

#[derive(Default)]
struct MockConfigCameraBackend {
    paths: Vec<String>,
    is_video: HashMap<String, bool>,
    formats: HashMap<String, Vec<u32>>,
}

impl ICameraBackend for MockConfigCameraBackend {
    fn get_device_paths(&self) -> Vec<String> {
        self.paths.clone()
    }

    fn is_video_capture_device(&self, path: &str) -> bool {
        self.is_video.get(path).copied().unwrap_or(false)
    }

    fn get_pixel_formats(&self, path: &str) -> Vec<u32> {
        self.formats.get(path).cloned().unwrap_or_default()
    }
}

#[test]
fn detects_ir_and_rgb() {
    let mut mock = MockConfigCameraBackend {
        paths: vec!["/dev/video0".to_string(), "/dev/video1".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video0".to_string(), true);
    mock.formats
        .insert("/dev/video0".to_string(), vec![V4L2_PIX_FMT_GREY]);
    mock.is_video.insert("/dev/video1".to_string(), true);
    mock.formats
        .insert("/dev/video1".to_string(), vec![V4L2_PIX_FMT_RGB24]);

    let mut config = Configuration::new();
    let input = Cursor::new(b"");
    assert!(config.load(input, Some(&mock)));

    assert_eq!(config.camera_defs.len(), 2);
    assert_eq!(config.camera_defs[0].id, "ir");
    assert_eq!(config.camera_defs[0].path, "/dev/video0");
    assert_eq!(config.camera_defs[1].id, "rgb");
    assert_eq!(config.camera_defs[1].path, "/dev/video1");
}

#[test]
fn detects_only_rgb() {
    let mut mock = MockConfigCameraBackend {
        paths: vec!["/dev/video1".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video1".to_string(), true);
    mock.formats
        .insert("/dev/video1".to_string(), vec![V4L2_PIX_FMT_MJPEG]);

    let mut config = Configuration::new();
    let input = Cursor::new(b"");
    assert!(config.load(input, Some(&mock)));

    assert_eq!(config.camera_defs.len(), 1);
    assert_eq!(config.camera_defs[0].id, "rgb");
    assert_eq!(config.camera_defs[0].path, "/dev/video1");
}

#[test]
fn no_cameras_detected() {
    let mock = MockConfigCameraBackend::default();
    let mut config = Configuration::new();
    let input = Cursor::new(b"");
    assert!(config.load(input, Some(&mock)));

    assert!(config.camera_defs.is_empty());
}

#[test]
fn ignore_already_configured_cameras() {
    let ini = b"[Cameras]\nnames=custom_cam\n\n[Camera.custom_cam]\npath=/dev/video99\n";
    let mut mock = MockConfigCameraBackend {
        paths: vec!["/dev/video0".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video0".to_string(), true);
    mock.formats
        .insert("/dev/video0".to_string(), vec![V4L2_PIX_FMT_GREY]);

    let mut config = Configuration::new();
    let input = Cursor::new(ini);
    assert!(config.load(input, Some(&mock)));

    assert_eq!(config.camera_defs.len(), 1);
    assert_eq!(config.camera_defs[0].id, "custom_cam");
    assert_eq!(config.camera_defs[0].path, "/dev/video99");
}
