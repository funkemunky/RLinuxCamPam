use std::path::Path;
use pam_linuxcampam::camera::{
    decode_v4l2_frame, Camera, ICamera, V4l2Buffer, V4l2Requestbuffers,
};
use pam_linuxcampam::utils::{
    V4L2_PIX_FMT_BGR24, V4L2_PIX_FMT_GREY, V4L2_PIX_FMT_NV12, V4L2_PIX_FMT_RGB24,
    V4L2_PIX_FMT_UYVY, V4L2_PIX_FMT_Y10, V4L2_PIX_FMT_Y12, V4L2_PIX_FMT_Y16, V4L2_PIX_FMT_YUYV,
};

#[test]
fn test_graceful_failure_on_invalid_device_path() {
    let mut cam = Camera::new("/dev/video9999", false, Some(Path::new("")));

    let frame = cam.capture();
    assert!(frame.is_none());

    let avg_frame = cam.capture_averaged(3);
    assert!(avg_frame.is_none());

    let hdr_frame = cam.capture_hdr();
    assert!(hdr_frame.is_none());

    assert!(!cam.supports_manual_exposure());
}

#[test]
fn test_trigger_ir_emitter_does_not_crash() {
    let mut cam = Camera::new("/dev/null", true, Some(Path::new("/bin/false")));
    // Should gracefully fail executing the command and not crash
    cam.trigger_ir_emitter();
}

#[test]
fn test_v4l2_struct_sizes() {
    assert_eq!(
        std::mem::size_of::<V4l2Requestbuffers>(),
        20,
        "V4l2Requestbuffers must be exactly 20 bytes matching Linux kernel"
    );
    assert_eq!(
        std::mem::size_of::<V4l2Buffer>(),
        88,
        "V4l2Buffer must be exactly 88 bytes matching Linux kernel on 64-bit"
    );
}

#[test]
fn test_decode_grey() {
    let data = [10u8, 20, 30, 40];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_GREY, 2).expect("decode failed");
    assert_eq!(img.width(), 2);
    assert_eq!(img.height(), 2);
    assert_eq!(img.get_pixel(0, 0).0, [10, 10, 10]);
    assert_eq!(img.get_pixel(1, 0).0, [20, 20, 20]);
    assert_eq!(img.get_pixel(0, 1).0, [30, 30, 30]);
    assert_eq!(img.get_pixel(1, 1).0, [40, 40, 40]);
}

#[test]
fn test_decode_rgb24() {
    let data = [
        255, 0, 0,    0, 255, 0,
        0, 0, 255,    255, 255, 255,
    ];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_RGB24, 6).expect("decode failed");
    assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(img.get_pixel(1, 0).0, [0, 255, 0]);
    assert_eq!(img.get_pixel(0, 1).0, [0, 0, 255]);
    assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255]);
}

#[test]
fn test_decode_bgr24() {
    let data = [
        0, 0, 255,    0, 255, 0,
        255, 0, 0,    255, 255, 255,
    ];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_BGR24, 6).expect("decode failed");
    assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(img.get_pixel(1, 0).0, [0, 255, 0]);
    assert_eq!(img.get_pixel(0, 1).0, [0, 0, 255]);
    assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255]);
}

#[test]
fn test_decode_yuyv() {
    // 2x2 image: each row has 2 pixels = 4 bytes of YUYV
    // Row 0: Y0=128, U=128, Y1=128, V=128 (neutral grey)
    // Row 1: Y0=235, U=128, Y1=235, V=128 (bright white)
    let data = [
        128, 128, 128, 128,
        235, 128, 235, 128,
    ];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_YUYV, 4).expect("decode failed");
    assert_eq!(img.width(), 2);
    assert_eq!(img.height(), 2);
    // Neutral grey Y=128, U=128, V=128 should give approximately [130, 130, 130]
    let p0 = img.get_pixel(0, 0).0;
    assert!(p0[0] > 100 && p0[0] < 150);
    assert_eq!(p0[0], p0[1]);
    assert_eq!(p0[1], p0[2]);
}

#[test]
fn test_decode_uyvy() {
    // 2x2 image: U, Y0, V, Y1
    let data = [
        128, 128, 128, 128,
        128, 235, 128, 235,
    ];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_UYVY, 4).expect("decode failed");
    assert_eq!(img.width(), 2);
    assert_eq!(img.height(), 2);
    let p0 = img.get_pixel(0, 0).0;
    assert!(p0[0] > 100 && p0[0] < 150);
}

#[test]
fn test_decode_y16_y12_y10() {
    // 2x2 image with 16-bit greyscale (2 bytes per pixel, 8 bytes total)
    let data = [
        0x00, 0x80, // 0x8000 -> 128
        0x00, 0xFF, // 0xFF00 -> 255
        0x00, 0x40, // 0x4000 -> 64
        0x00, 0x00, // 0x0000 -> 0
    ];
    let img16 = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_Y16, 4).expect("decode Y16 failed");
    assert_eq!(img16.get_pixel(0, 0).0, [128, 128, 128]);
    assert_eq!(img16.get_pixel(1, 0).0, [255, 255, 255]);
    assert_eq!(img16.get_pixel(0, 1).0, [64, 64, 64]);
    assert_eq!(img16.get_pixel(1, 1).0, [0, 0, 0]);

    // Test Y12 and Y10
    let img12 = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_Y12, 4).expect("decode Y12 failed");
    assert_eq!(img12.width(), 2);
    let img10 = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_Y10, 4).expect("decode Y10 failed");
    assert_eq!(img10.width(), 2);
}

#[test]
fn test_decode_nv12() {
    // 2x2 image: 4 Y bytes + 2 UV bytes
    let data = [
        128, 128,
        128, 128,
        128, 128,
    ];
    let img = decode_v4l2_frame(&data, 2, 2, V4L2_PIX_FMT_NV12, 2).expect("decode NV12 failed");
    assert_eq!(img.width(), 2);
    assert_eq!(img.height(), 2);
}

#[test]
fn test_real_camera_capture_if_present() {
    // If real video devices exist on the host, test capturing a frame!
    if Path::new("/dev/video2").exists() {
        let mut cam_ir = Camera::new("/dev/video2", true, None);
        let frame_ir = cam_ir.capture();
        assert!(
            frame_ir.is_some(),
            "IR camera capture on /dev/video2 should succeed and return Some(RgbImage)"
        );
        let f = frame_ir.unwrap();
        assert!(f.width() > 0 && f.height() > 0);

        // Also test averaged capture (used during enrollment)
        let frame_ir_avg = cam_ir.capture_averaged(3);
        assert!(
            frame_ir_avg.is_some(),
            "IR camera capture_averaged on /dev/video2 should succeed and return Some(RgbImage)"
        );
        let f_avg = frame_ir_avg.unwrap();
        assert_eq!(f_avg.width(), f.width());
        assert_eq!(f_avg.height(), f.height());
    }

    if Path::new("/dev/video0").exists() {
        let mut cam_rgb = Camera::new("/dev/video0", false, None);
        let frame_rgb = cam_rgb.capture();
        assert!(
            frame_rgb.is_some(),
            "RGB camera capture on /dev/video0 should succeed and return Some(RgbImage)"
        );
        let f = frame_rgb.unwrap();
        assert!(f.width() > 0 && f.height() > 0);

        // Also test averaged capture
        let frame_rgb_avg = cam_rgb.capture_averaged(3);
        assert!(
            frame_rgb_avg.is_some(),
            "RGB camera capture_averaged on /dev/video0 should succeed and return Some(RgbImage)"
        );

        // Also test HDR capture
        let frame_rgb_hdr = cam_rgb.capture_hdr();
        assert!(
            frame_rgb_hdr.is_some(),
            "RGB camera capture_hdr on /dev/video0 should succeed and return Some(RgbImage)"
        );
    }
}

#[test]
fn test_auth_engine_camera_capture() {
    let mut engine = pam_linuxcampam::auth_engine::AuthEngine::new();
    let config_path = Path::new("config/config.ini");
    if config_path.exists() {
        let _ = engine.init(config_path);

        // Setup a temporary user directory with an enrolled user so verify_user_with_details
        // actually triggers the real camera capture loop instead of returning early.
        let temp_dir = tempfile::tempdir().expect("tempdir failed");
        engine.get_config_mut().users_dir = temp_dir.path().to_path_buf();

        let user_file = temp_dir.path().join("real_test_user.json");
        let dummy_emb: Vec<f32> = vec![0.1; 128];
        let user_json = serde_json::json!({
            "username": "real_test_user",
            "created": 123456789,
            "embeddings_ir": [{"label": "default", "data": dummy_emb}],
            "embeddings_rgb": [{"label": "default", "data": dummy_emb}]
        });
        std::fs::write(&user_file, serde_json::to_string(&user_json).unwrap()).unwrap();

        let result = engine.verify_user_with_details("real_test_user");
        assert_ne!(
            result.reason, "Mandatory Camera ir failed",
            "Camera capture should succeed, failing only on face recognition/matching"
        );
        assert_ne!(
            result.reason, "Camera ir failed to capture",
            "Camera capture should succeed"
        );
        assert_ne!(
            result.reason, "User not enrolled or corrupt data",
            "User was enrolled so data must load properly"
        );
    }
}

#[test]
fn test_auth_engine_enroll_real_cameras() {
    let mut engine = pam_linuxcampam::auth_engine::AuthEngine::new();
    let config_path = Path::new("config/config.ini");
    if config_path.exists() {
        let _ = engine.init(config_path);
        let temp_dir = tempfile::tempdir().expect("tempdir failed");
        engine.get_config_mut().users_dir = temp_dir.path().to_path_buf();

        let (_ok, msg) = engine.enroll_user("enroll_test_user");
        assert_ne!(
            msg, "Camera ir failed (empty frame).",
            "IR camera capture during enrollment must not fail with empty frame"
        );
        assert!(!msg.contains("failed (empty frame)"), "No camera should fail with empty frame: {msg}");
    }
}

