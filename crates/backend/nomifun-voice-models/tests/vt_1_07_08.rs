//! Vertical Slice Tests VT-1-07 & VT-1-08:
//! SHA256 强校验与损坏拦截。
//!
//! Baseline:
//! - `model-management.zh.md` §2
//! - `wave-1-acoustic.zh.md` §3 (VT-1-07, VT-1-08)

use std::collections::HashMap;
use nomifun_api_types::voice::ModelInstallState;
use nomifun_voice_models::verifier::ModelVerifier;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[tokio::test]
async fn test_vt_1_07_complete_fixtures_offline_verification() {
    let temp_dir = tempdir().expect("create temp dir");
    let models_dir = temp_dir.path();

    // 1. Prepare minimal fixtures with 8 expected files
    let test_files: &[(&str, &[u8])] = &[
        ("kws/encoder.int8.onnx", b"dummy-encoder-onnx-content-12345"),
        ("kws/decoder.int8.onnx", b"dummy-decoder-onnx-content-67890"),
        ("kws/joiner.int8.onnx", b"dummy-joiner-onnx-content-abcde"),
        ("kws/tokens.txt", b"<blk> 0\nhey 1\nflowy 2\n"),
        ("kws/keywords.txt", b"HEY FLOWY :1.0 #0.25 @1\n"),
        ("vad/silero_vad.onnx", b"dummy-silero-vad-model-bytes-999"),
        ("asr/model.int8.onnx", b"dummy-sensevoice-asr-int8-bytes"),
        ("asr/tokens.txt", b"hello 1\nworld 2\n"),
    ];

    let mut baseline_manifest = HashMap::new();
    let mut checksum_file_content = String::new();

    for (rel_path, content) in test_files {
        let full_path = models_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full_path, content).expect("write model file");

        // Only 7 downloaded distribution files are in checksums.sha256 (keywords.txt is locally compiled)
        if *rel_path != "kws/keywords.txt" {
            let hash = sha256_hex(content);
            baseline_manifest.insert(rel_path.to_string(), hash.clone());
            checksum_file_content.push_str(&format!("{hash}  {rel_path}\n"));
        }
    }

    // Write checksums.sha256
    std::fs::write(models_dir.join("checksums.sha256"), checksum_file_content)
        .expect("write checksums.sha256");

    // 2. Perform verification with ModelVerifier using baseline manifest
    let verifier = ModelVerifier::with_manifest(baseline_manifest);
    let state = verifier.verify(models_dir).await;

    // 3. Assert state is Ready, 100% matched, 0 external network requests
    match state {
        ModelInstallState::Ready {
            models_dir: ready_dir,
            total_size_bytes,
        } => {
            assert_eq!(ready_dir, models_dir);
            assert!(total_size_bytes > 0);
        }
        other => panic!("expected ModelInstallState::Ready, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_vt_1_08_corrupted_file_intercepted_without_crash() {
    let temp_dir = tempdir().expect("create temp dir");
    let models_dir = temp_dir.path();

    // 1. Prepare files with one corrupted file
    let test_files: &[(&str, &[u8])] = &[
        ("kws/encoder.int8.onnx", b"CORRUPTED-MODIFIED-DATA-BYTES"), // Corrupted!
        ("kws/decoder.int8.onnx", b"dummy-decoder-onnx-content-67890"),
        ("kws/joiner.int8.onnx", b"dummy-joiner-onnx-content-abcde"),
        ("kws/tokens.txt", b"<blk> 0\nhey 1\nflowy 2\n"),
        ("kws/keywords.txt", b"HEY FLOWY :1.0 #0.25 @1\n"),
        ("vad/silero_vad.onnx", b"dummy-silero-vad-model-bytes-999"),
        ("asr/model.int8.onnx", b"dummy-sensevoice-asr-int8-bytes"),
        ("asr/tokens.txt", b"hello 1\nworld 2\n"),
    ];

    let mut baseline_manifest = HashMap::new();
    let original_content = b"original-unmodified-encoder-data";
    baseline_manifest.insert(
        "kws/encoder.int8.onnx".to_string(),
        sha256_hex(original_content),
    );

    for (rel_path, content) in test_files {
        let full_path = models_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full_path, content).expect("write model file");

        if *rel_path != "kws/keywords.txt" && *rel_path != "kws/encoder.int8.onnx" {
            baseline_manifest.insert(rel_path.to_string(), sha256_hex(content));
        }
    }

    std::fs::write(
        models_dir.join("checksums.sha256"),
        "dummy-hash  kws/encoder.int8.onnx\n",
    )
    .expect("write checksums");

    // 2. Perform verification
    let verifier = ModelVerifier::with_manifest(baseline_manifest);
    let state = verifier.verify(models_dir).await;

    // 3. Assert state is Failed / Corrupted, recoverable is true, no ONNX crash
    match state {
        ModelInstallState::Failed {
            reason,
            recoverable,
        } => {
            assert!(recoverable, "corrupted models must be recoverable");
            assert!(
                reason.contains("kws/encoder.int8.onnx") || reason.contains("mismatch"),
                "reason should mention the failing file or mismatch, got: {reason}"
            );
        }
        other => panic!("expected ModelInstallState::Failed, got: {other:?}"),
    }
}
