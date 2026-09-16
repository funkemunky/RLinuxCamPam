use serde_json::json;

fn is_valid_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 32 {
        return false;
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[test]
fn valid_labels() {
    assert!(is_valid_label("default"));
    assert!(is_valid_label("daylight"));
    assert!(is_valid_label("glasses-on"));
    assert!(is_valid_label("low_light_2"));
}

#[test]
fn invalid_labels() {
    assert!(!is_valid_label(""));
    assert!(!is_valid_label("label with spaces"));
    assert!(!is_valid_label("label/slash"));
    assert!(!is_valid_label("label..dots"));
    assert!(!is_valid_label("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")); // >32 chars
}

#[test]
fn single_embedding_structure() {
    let entry = json!({
        "label": "default",
        "data": [0.1f32, 0.2f32, 0.3f32],
        "created": 123456789u64,
        "model_version": "sface_2021dec",
    });

    assert!(entry.get("label").is_some());
    assert!(entry.get("data").is_some());
    assert!(entry.get("created").is_some());
    assert!(entry.get("model_version").is_some());
    assert_eq!(entry["label"], "default");
    assert_eq!(entry["model_version"], "sface_2021dec");
}

#[test]
fn multi_embedding_array() {
    let mut embeddings = Vec::new();
    embeddings.push(json!({
        "label": "default",
        "data": [0.1f32, 0.2f32],
        "model_version": "sface_2021dec",
    }));
    embeddings.push(json!({
        "label": "glasses",
        "data": [0.3f32, 0.4f32],
        "model_version": "sface_2021dec",
    }));

    assert_eq!(embeddings.len(), 2);
    assert_eq!(embeddings[0]["label"], "default");
    assert_eq!(embeddings[1]["label"], "glasses");
}

#[test]
fn user_file_structure() {
    let mut user_data = json!({
        "embeddings_ir": [],
        "embeddings_rgb": []
    });

    let ir_emb = json!({
        "label": "default",
        "data": vec![0.5f32; 128],
        "model_version": "sface_2021dec"
    });

    user_data["embeddings_ir"]
        .as_array_mut()
        .unwrap()
        .push(ir_emb);

    assert!(user_data.get("embeddings_ir").is_some());
    assert!(user_data.get("embeddings_rgb").is_some());
    assert_eq!(user_data["embeddings_ir"].as_array().unwrap().len(), 1);
    assert_eq!(user_data["embeddings_rgb"].as_array().unwrap().len(), 0);
}

#[test]
fn parse_embedding_data() {
    let json_str = r#"{
        "label": "test",
        "data": [0.1, 0.2, 0.3, 0.4, 0.5],
        "model_version": "sface_2021dec"
    }"#;

    let entry: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let data: Vec<f32> = entry["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();

    assert_eq!(data.len(), 5);
    assert_eq!(data[0], 0.1);
    assert_eq!(data[4], 0.5);
}
