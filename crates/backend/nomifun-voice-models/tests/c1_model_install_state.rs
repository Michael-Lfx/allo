use std::path::PathBuf;
use nomifun_voice_models::ModelInstallState;
use serde_json::json;

#[test]
fn test_c1_01_not_installed_snapshot() {
    let state = ModelInstallState::NotInstalled;
    let expected_json = json!({
        "status": "NotInstalled"
    });

    let serialized = serde_json::to_value(&state).expect("serialize NotInstalled");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize NotInstalled");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_02_probing_source_snapshot() {
    let state = ModelInstallState::ProbingSource;
    let expected_json = json!({
        "status": "ProbingSource"
    });

    let serialized = serde_json::to_value(&state).expect("serialize ProbingSource");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize ProbingSource");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_03_downloading_snapshot() {
    let raw_golden_json = r#"{
        "status": "Downloading",
        "detail": {
            "current_file": "kws/encoder.int8.onnx",
            "file_index": 1,
            "total_files": 7,
            "downloaded_bytes": 1048576,
            "total_bytes": 17825792,
            "progress_percent": 5.88,
            "speed_bytes_per_sec": 1048576
        }
    }"#;

    // 1. Verify deserialization from golden wire JSON
    let deserialized: ModelInstallState =
        serde_json::from_str(raw_golden_json).expect("deserialize golden Downloading JSON");

    match &deserialized {
        ModelInstallState::Downloading {
            current_file,
            file_index,
            total_files,
            downloaded_bytes,
            total_bytes,
            progress_percent,
            speed_bytes_per_sec,
        } => {
            assert_eq!(current_file, "kws/encoder.int8.onnx");
            assert_eq!(*file_index, 1);
            assert_eq!(*total_files, 7);
            assert_eq!(*downloaded_bytes, 1_048_576);
            assert_eq!(*total_bytes, 17_825_792);
            assert!((progress_percent - 5.88).abs() < 1e-4);
            assert_eq!(*speed_bytes_per_sec, 1_048_576);
        }
        other => panic!("expected Downloading, got {other:?}"),
    }

    // 2. Verify serialization matches expected tag/content structure
    let serialized_val = serde_json::to_value(&deserialized).expect("serialize Downloading");
    assert_eq!(serialized_val["status"], "Downloading");
    assert_eq!(serialized_val["detail"]["current_file"], "kws/encoder.int8.onnx");
    assert_eq!(serialized_val["detail"]["file_index"], 1);
    assert_eq!(serialized_val["detail"]["total_files"], 7);
    assert_eq!(serialized_val["detail"]["downloaded_bytes"], 1048576);
    assert_eq!(serialized_val["detail"]["total_bytes"], 17825792);
    assert_eq!(serialized_val["detail"]["speed_bytes_per_sec"], 1048576);

    // 3. Verify round-trip serialization/deserialization
    let roundtrip: ModelInstallState =
        serde_json::from_value(serialized_val).expect("roundtrip deserialize");
    assert_eq!(roundtrip, deserialized);
}

#[test]
fn test_c1_04_verifying_sha256_snapshot() {
    let state = ModelInstallState::VerifyingSha256 {
        current_file: "vad/silero_vad.onnx".to_string(),
        verified_files: 2,
        total_files: 7,
    };

    let expected_json = json!({
        "status": "VerifyingSha256",
        "detail": {
            "current_file": "vad/silero_vad.onnx",
            "verified_files": 2,
            "total_files": 7
        }
    });

    let serialized = serde_json::to_value(&state).expect("serialize VerifyingSha256");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize VerifyingSha256");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_05_ready_snapshot() {
    let models_dir = PathBuf::from("/var/lib/ai-mate/models");
    let state = ModelInstallState::Ready {
        models_dir: models_dir.clone(),
        total_size_bytes: 261_095_424,
    };

    let expected_json = json!({
        "status": "Ready",
        "detail": {
            "models_dir": models_dir.to_string_lossy(),
            "total_size_bytes": 261095424
        }
    });

    let serialized = serde_json::to_value(&state).expect("serialize Ready");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize Ready");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_06_failed_snapshot() {
    let state = ModelInstallState::Failed {
        reason: "SHA256 checksum mismatch for asr/model.int8.onnx".to_string(),
        recoverable: true,
    };

    let expected_json = json!({
        "status": "Failed",
        "detail": {
            "reason": "SHA256 checksum mismatch for asr/model.int8.onnx",
            "recoverable": true
        }
    });

    let serialized = serde_json::to_value(&state).expect("serialize Failed");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize Failed");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_07_removing_snapshot() {
    let state = ModelInstallState::Removing;
    let expected_json = json!({
        "status": "Removing"
    });

    let serialized = serde_json::to_value(&state).expect("serialize Removing");
    assert_eq!(serialized, expected_json);

    let deserialized: ModelInstallState = serde_json::from_value(expected_json)
        .expect("deserialize Removing");
    assert_eq!(deserialized, state);
}

#[test]
fn test_c1_08_unknown_status_fails_deserialization() {
    let raw = json!({
        "status": "NonExistentStatus"
    });

    let res = serde_json::from_value::<ModelInstallState>(raw);
    assert!(res.is_err(), "unknown status must fail deserialization");
}
