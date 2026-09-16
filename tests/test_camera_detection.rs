use std::collections::HashMap;
use std::time::{Duration, Instant};

use pam_linuxcampam::utils::{
    classify_camera_type, enumerate_cameras, poll_remaining_ms, ICameraBackend,
    V4L2_PIX_FMT_GREY, V4L2_PIX_FMT_MJPEG, V4L2_PIX_FMT_RGB24,
};

#[derive(Default)]
struct MockCameraBackend {
    paths: Vec<String>,
    is_video: HashMap<String, bool>,
    formats: HashMap<String, Vec<u32>>,
}

impl ICameraBackend for MockCameraBackend {
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
fn no_devices() {
    let mock = MockCameraBackend::default();
    let cameras = enumerate_cameras(&mock);
    assert!(cameras.is_empty());
}

#[test]
fn ignore_non_video_devices() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video0".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video0".to_string(), false);

    let cameras = enumerate_cameras(&mock);
    assert!(cameras.is_empty());
}

#[test]
fn detect_ir_camera() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video0".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video0".to_string(), true);
    mock.formats
        .insert("/dev/video0".to_string(), vec![V4L2_PIX_FMT_GREY]);

    let cameras = enumerate_cameras(&mock);
    assert_eq!(cameras.len(), 1);
    assert_eq!(cameras[0].0, "/dev/video0");
    assert_eq!(cameras[0].1, "ir");
}

#[test]
fn detect_rgb_camera() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video1".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video1".to_string(), true);
    mock.formats
        .insert("/dev/video1".to_string(), vec![V4L2_PIX_FMT_RGB24]);

    let cameras = enumerate_cameras(&mock);
    assert_eq!(cameras.len(), 1);
    assert_eq!(cameras[0].0, "/dev/video1");
    assert_eq!(cameras[0].1, "rgb");
}

#[test]
fn detect_generic_camera() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video2".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video2".to_string(), true);
    mock.formats.insert("/dev/video2".to_string(), vec![0]); // Unknown format

    let cameras = enumerate_cameras(&mock);
    assert_eq!(cameras.len(), 1);
    assert_eq!(cameras[0].0, "/dev/video2");
    assert_eq!(cameras[0].1, "generic");
}

#[test]
fn detect_combined_camera_as_rgb() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video3".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video3".to_string(), true);
    mock.formats.insert(
        "/dev/video3".to_string(),
        vec![V4L2_PIX_FMT_GREY, V4L2_PIX_FMT_RGB24],
    );

    let cam_type = classify_camera_type("/dev/video3", &mock);
    assert_eq!(cam_type, "rgb");
}

#[test]
fn detect_mixed_devices() {
    let mut mock = MockCameraBackend {
        paths: vec!["/dev/video0".to_string(), "/dev/video1".to_string()],
        ..Default::default()
    };
    mock.is_video.insert("/dev/video0".to_string(), true);
    mock.formats
        .insert("/dev/video0".to_string(), vec![V4L2_PIX_FMT_GREY]);
    mock.is_video.insert("/dev/video1".to_string(), true);
    mock.formats
        .insert("/dev/video1".to_string(), vec![V4L2_PIX_FMT_MJPEG]);

    let cameras = enumerate_cameras(&mock);
    assert_eq!(cameras.len(), 2);
    assert_eq!(cameras[0].0, "/dev/video0");
    assert_eq!(cameras[0].1, "ir");
    assert_eq!(cameras[1].0, "/dev/video1");
    assert_eq!(cameras[1].1, "rgb");
}

#[test]
fn poll_remaining_returns_full_window_when_deadline_is_ahead() {
    let now = Instant::now();
    let deadline = now + Duration::from_millis(5000);
    assert_eq!(poll_remaining_ms(deadline, now), 5000);
}

#[test]
fn poll_remaining_shrinks_as_time_elapses() {
    let now = Instant::now();
    let deadline = now + Duration::from_millis(5000);
    let later = now + Duration::from_millis(3200);
    assert_eq!(poll_remaining_ms(deadline, later), 1800);
}

#[test]
fn poll_remaining_clamps_to_zero_at_deadline() {
    let now = Instant::now();
    assert_eq!(poll_remaining_ms(now, now), 0);
}

#[test]
fn poll_remaining_clamps_to_zero_past_deadline() {
    let now = Instant::now();
    let deadline = now - Duration::from_millis(1500);
    assert_eq!(poll_remaining_ms(deadline, now), 0);
}
