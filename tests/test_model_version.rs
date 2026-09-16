use pam_linuxcampam::utils::get_model_version;

#[test]
fn standard_sface_model() {
    assert_eq!(
        get_model_version("/etc/linuxcampam/models/face_recognition_sface_2021dec.onnx"),
        "sface_2021dec"
    );
}

#[test]
fn standard_yunet_model() {
    assert_eq!(
        get_model_version("/etc/linuxcampam/models/face_detection_yunet_2023mar.onnx"),
        "face_detection_yunet_2023mar"
    );
}

#[test]
fn future_model_version() {
    assert_eq!(
        get_model_version("/path/to/face_recognition_sface_2024.onnx"),
        "sface_2024"
    );
}

#[test]
fn custom_model_without_prefix() {
    assert_eq!(
        get_model_version("/models/custom_recognizer_v2.onnx"),
        "custom_recognizer_v2"
    );
}

#[test]
fn relative_path() {
    assert_eq!(
        get_model_version("models/face_recognition_arcface.onnx"),
        "arcface"
    );
}

#[test]
fn just_filename() {
    assert_eq!(
        get_model_version("face_recognition_vggface.onnx"),
        "vggface"
    );
}

#[test]
fn no_extension() {
    assert_eq!(get_model_version("/path/face_recognition_test"), "test");
}
