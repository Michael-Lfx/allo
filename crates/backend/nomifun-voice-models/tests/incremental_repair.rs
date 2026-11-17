use std::{collections::HashMap, sync::{Arc, atomic::{AtomicUsize, Ordering}}};
use nomifun_voice_models::{ModelManager, MirrorConfig, EXPECTED_FILES, VoiceModelInstallRequest};
use nomifun_voice_models::verifier::{compute_sha256, DOWNLOAD_FILES};
use nomifun_voice_models::downloader::{ModelDownloader, transaction_paths};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture(root: &std::path::Path) -> HashMap<String,String> {
    let mut hashes = HashMap::new();
    for rel in EXPECTED_FILES {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = format!("fixture bytes: {rel}");
        std::fs::write(&path, &bytes).unwrap();
        if DOWNLOAD_FILES.contains(rel) { hashes.insert(rel.to_string(), compute_sha256(bytes.as_bytes())); }
    }
    hashes
}
fn mirror(url: String) -> MirrorConfig {
    MirrorConfig { id: "loopback".into(), base_url: url, probe_url: None, is_modelscope:false, is_fallback_cdn:false }
}
async fn server(bytes: Vec<u8>) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0;4096];
            stream.read(&mut request).await.unwrap();
            hits.fetch_add(1, Ordering::SeqCst);
            let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&bytes).await.unwrap();
        }
    });
    (url,count,task)
}

async fn path_server(
    responses: HashMap<String, Option<Vec<u8>>>,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0;4096];
            let read = stream.read(&mut request).await.unwrap();
            hits.fetch_add(1, Ordering::SeqCst);
            let text = String::from_utf8_lossy(&request[..read]);
            let path = text
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .trim_start_matches('/')
                .to_string();
            match responses.get(&path) {
                Some(Some(bytes)) => {
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
                    stream.write_all(header.as_bytes()).await.unwrap();
                    stream.write_all(bytes).await.unwrap();
                }
                _ => {
                    stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                }
            }
        }
    });
    (url,count,task)
}


#[tokio::test]
async fn old_force_request_rehashes_without_get_or_model_writes() {
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    let before: Vec<_> = DOWNLOAD_FILES.iter().map(|p| std::fs::metadata(dir.path().join(p)).unwrap().modified().unwrap()).collect();
    let (url,count,task) = server(b"must not download".to_vec()).await;
    let manager = ModelManager::with_config(dir.path(), vec![mirror(url)], hashes);
    let req: VoiceModelInstallRequest = serde_json::from_str(r#"{"force":true}"#).unwrap();
    assert!(manager.install(Some(req)).await.unwrap().accepted);
    assert!(manager.wait_for_completion().await.is_ready());
    assert_eq!(count.load(Ordering::SeqCst),0);
    for (i,p) in DOWNLOAD_FILES.iter().enumerate() {
        assert_eq!(std::fs::metadata(dir.path().join(p)).unwrap().modified().unwrap(), before[i]);
    }
    task.abort();
}

#[tokio::test]
async fn one_corrupt_file_repairs_only_that_file_and_retry_is_zero_get() {
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    let rel = "asr/model.int8.onnx";
    let expected = std::fs::read(dir.path().join(rel)).unwrap();
    std::fs::write(dir.path().join(rel), b"broken").unwrap();
    let (url,count,task) = server(expected.clone()).await;
    let manager = ModelManager::with_config(dir.path(),vec![mirror(url)],hashes);
    manager.install(None).await.unwrap();
    assert!(manager.wait_for_completion().await.is_ready());
    assert_eq!(count.load(Ordering::SeqCst),1);
    manager.install(None).await.unwrap();
    assert!(manager.wait_for_completion().await.is_ready());
    assert_eq!(count.load(Ordering::SeqCst),1);
    assert_eq!(std::fs::read(dir.path().join(rel)).unwrap(),expected);
    task.abort();
}

#[tokio::test]
async fn wrong_hash_retains_old_target_and_recovery_promotes_verified_part() {
    let dir=tempfile::tempdir().unwrap();
    let target=dir.path().join("model.onnx");
    std::fs::write(&target,b"old data").unwrap();
    let desired=b"new model";
    let hash=compute_sha256(desired);
    let (url,_,task)=server(b"<html>bad</html>".to_vec()).await;
    let downloader=ModelDownloader::new(vec![mirror(url)]);
    assert!(downloader.download_verified("model.onnx",&target,&hash,Some(desired.len() as u64)).await.is_err());
    assert_eq!(std::fs::read(&target).unwrap(),b"old data");
    let [_,part,backup]=transaction_paths(&target,&hash,desired.len() as u64);
    std::fs::rename(&target,&backup).unwrap();
    std::fs::write(&part,desired).unwrap();
    let offline=ModelDownloader::new(vec![]);
    offline.download_verified("model.onnx",&target,&hash,Some(desired.len() as u64)).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(),desired);
    assert!(!backup.exists());
    task.abort();
}

#[tokio::test]
async fn restored_valid_backup_needs_no_source_or_get() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("asset.onnx");
    let bytes = b"valid recoverable model";
    let hash = compute_sha256(bytes);
    let [_, _, backup] = transaction_paths(&target, &hash, bytes.len() as u64);
    std::fs::write(&backup, bytes).unwrap();
    ModelDownloader::new(vec![]).download_verified("asset.onnx", &target, &hash, Some(bytes.len() as u64)).await.unwrap();
    assert_eq!(std::fs::read(target).unwrap(), bytes);
    assert!(!backup.exists());
}

#[tokio::test]
async fn ambiguous_kws_requires_separate_confirmation_and_preserves_keywords() {
    let dir = tempfile::tempdir().unwrap();
    let mut hashes = fixture(dir.path());
    let keywords = std::fs::read(dir.path().join("kws/keywords.txt")).unwrap();
    let expected = b"confirmed replacement";
    for path in DOWNLOAD_FILES.iter().filter(|p| p.starts_with("kws/")) {
        hashes.insert(path.to_string(), compute_sha256(expected));
        std::fs::write(dir.path().join(path), b"unidentified KWS").unwrap();
    }
    let (url,count,task) = server(expected.to_vec()).await;
    let manager = ModelManager::with_config(dir.path(),vec![mirror(url)],hashes);
    let force = serde_json::from_str(r#"{"force":true}"#).unwrap();
    manager.install(Some(force)).await.unwrap();
    assert!(manager.wait_for_completion().await.is_failed());
    assert_eq!(count.load(Ordering::SeqCst),0);
    let confirmed = serde_json::from_str(r#"{"force":true,"rebuild_default_kws":true}"#).unwrap();
    manager.install(Some(confirmed)).await.unwrap();
    assert!(manager.wait_for_completion().await.is_ready());
    assert_eq!(count.load(Ordering::SeqCst),4);
    assert_eq!(std::fs::read(dir.path().join("kws/keywords.txt")).unwrap(),keywords);
    task.abort();
}

#[tokio::test]
async fn hash_failure_falls_back_without_touching_old_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("asset.onnx");
    std::fs::write(&target, b"old model").unwrap();
    let expected = b"expected model";
    let (bad_url, bad_count, bad_task) = server(b"wrong resource".to_vec()).await;
    let (good_url, good_count, good_task) = server(expected.to_vec()).await;
    let downloader = ModelDownloader::new(vec![mirror(bad_url), mirror(good_url)]);
    downloader.download_verified("asset.onnx", &target, &compute_sha256(expected), Some(expected.len() as u64)).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), expected);
    assert_eq!(bad_count.load(Ordering::SeqCst), 1);
    assert_eq!(good_count.load(Ordering::SeqCst), 1);
    bad_task.abort();
    good_task.abort();
}

#[tokio::test]
async fn uninstall_removes_owned_recovery_artifacts_without_reviving_data() {
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    let path = "asr/model.int8.onnx";
    let artifacts = transaction_paths(&dir.path().join(path), &hashes[path], 0);
    for artifact in &artifacts { std::fs::write(artifact, b"old interrupted transaction").unwrap(); }
    std::fs::write(dir.path().join("keep-user-file"), b"keep").unwrap();
    let manager = ModelManager::with_config(dir.path(), vec![], hashes);
    assert!(manager.remove().await.unwrap().success);
    assert!(artifacts.iter().all(|p| !p.exists()));
    assert!(dir.path().join("keep-user-file").exists());
    assert!(!ModelManager::new(dir.path()).get_status().await.state.is_ready());
}

#[test]
fn kws_complete_groups_are_retained_and_cross_group_mixture_is_rejected() {
    use nomifun_voice_models::verifier::{embedded_manifest, matching_kws_group, validate_keywords};
    let groups = &embedded_manifest().kws_groups;
    for group in groups { assert_eq!(matching_kws_group(&group.files).unwrap().group_id, group.group_id); }
    let mut mixed = groups[0].files.clone();
    mixed.insert("kws/encoder.int8.onnx".into(), groups[1].files["kws/encoder.int8.onnx"].clone());
    assert!(matching_kws_group(&mixed).is_none());
    assert!(validate_keywords("▁HE 1\nY 2\n", "▁HE Y :1.5 #0.15 @1\n").is_ok());
    assert!(validate_keywords("▁HE 1\n", "▁HE Y :1.5 #0.15 @1\n").is_err());
}

#[test]
fn manifest_sources_are_fixed_and_groups_are_complete() {
    let manifest=nomifun_voice_models::verifier::embedded_manifest();
    assert_eq!(manifest.files.len(),23);
    for file in &manifest.files {
        let url=file.url.as_ref().unwrap();
        assert!(url.contains(file.revision.as_ref().unwrap()));
        assert!(!url.contains("/main/") && !url.contains("Revision=master"));
        assert_eq!(file.sha256.len(),64);
        assert!(file.size>0);
    }
    assert!(manifest.kws_groups.iter().all(|g|g.files.len()==4));
}

#[tokio::test]
async fn confirmed_rebuild_intent_survives_failure_and_plain_retry_finishes() {
    use nomifun_voice_models::intent::{REBUILD_INTENT_REL, REBUILD_INTENT_TMP_REL};
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    let desired: HashMap<String, Vec<u8>> = DOWNLOAD_FILES
        .iter()
        .map(|path| ((*path).to_string(), std::fs::read(dir.path().join(path)).unwrap()))
        .collect();
    let keywords = b"user custom wake phrase\n";
    std::fs::write(dir.path().join("kws/keywords.txt"), keywords).unwrap();
    // An old, partial group: every KWS file is unknown to the default identity
    // and the decoder is missing entirely.
    for path in DOWNLOAD_FILES.iter().filter(|path| path.starts_with("kws/")) {
        std::fs::write(dir.path().join(path), b"old group bytes").unwrap();
    }
    std::fs::remove_file(dir.path().join("kws/decoder.int8.onnx")).unwrap();
    nomifun_voice_models::intent::persist_default_rebuild(dir.path()).unwrap();
    assert!(nomifun_voice_models::intent::confirmed_default_rebuild(dir.path()).unwrap());

    let mut responses = HashMap::new();
    for (path, bytes) in &desired {
        responses.insert(path.clone(), Some(bytes.clone()));
    }
    responses.insert("kws/decoder.int8.onnx".into(), None);
    let (first_url, _, first_task) = path_server(responses).await;
    let first = ModelManager::with_config(dir.path(), vec![mirror(first_url)], hashes.clone());
    assert!(first.install(None).await.unwrap().accepted);
    assert!(first.wait_for_completion().await.is_failed());
    assert!(dir.path().join(REBUILD_INTENT_REL).is_file(), "a failed rebuild must retain its confirmed target");

    let (second_url, _, second_task) = path_server(
        desired.iter().map(|(path, bytes)| (path.clone(), Some(bytes.clone()))).collect(),
    )
    .await;
    let second = ModelManager::with_config(dir.path(), vec![mirror(second_url)], hashes);
    // No rebuild_default_kws flag: an ordinary retry must follow the intent.
    assert!(second.install(None).await.unwrap().accepted);
    assert!(second.wait_for_completion().await.is_ready());
    assert!(!dir.path().join(REBUILD_INTENT_REL).exists());
    assert!(!dir.path().join(REBUILD_INTENT_TMP_REL).exists());
    assert_eq!(std::fs::read(dir.path().join("kws/keywords.txt")).unwrap(), keywords);
    for (path, bytes) in desired {
        assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), bytes, "{path}");
    }
    first_task.abort();
    second_task.abort();
}

#[tokio::test]
async fn mixed_kws_without_intent_blocks_before_any_download() {
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    std::fs::write(dir.path().join("kws/encoder.int8.onnx"), b"foreign encoder").unwrap();
    let (url, count, task) = server(b"must not be requested".to_vec()).await;
    let manager = ModelManager::with_config(dir.path(), vec![mirror(url)], hashes);
    assert!(manager.install(None).await.unwrap().accepted);
    assert!(manager.wait_for_completion().await.is_failed());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(dir.path().join("kws/encoder.int8.onnx")).unwrap(), b"foreign encoder");
    assert!(!dir.path().join("kws/.voice-suite-rebuild-intent").exists());
    task.abort();
}

#[tokio::test]
async fn uninstall_removes_rebuild_intent_and_staging_copy() {
    use nomifun_voice_models::intent::{REBUILD_INTENT_REL, REBUILD_INTENT_TMP_REL};
    let dir = tempfile::tempdir().unwrap();
    let hashes = fixture(dir.path());
    nomifun_voice_models::intent::persist_default_rebuild(dir.path()).unwrap();
    std::fs::write(dir.path().join(REBUILD_INTENT_TMP_REL), b"stale").unwrap();
    let manager = ModelManager::with_config(dir.path(), vec![], hashes);
    assert!(manager.remove().await.unwrap().success);
    assert!(!dir.path().join(REBUILD_INTENT_REL).exists());
    assert!(!dir.path().join(REBUILD_INTENT_TMP_REL).exists());
}

#[test]
fn rust_and_python_share_the_rebuild_intent_document_shape() {
    let manifest = nomifun_voice_models::verifier::embedded_manifest();
    let default = nomifun_voice_models::verifier::default_kws_group();
    let document: serde_json::Value =
        serde_json::from_str(&nomifun_voice_models::intent::default_document()).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["group_id"], default.group_id);
    assert_eq!(document["files"].as_object().unwrap().len(), 4);
    assert_eq!(document["files"].as_object().unwrap().len(), default.files.len());
    assert!(default.files.iter().all(|(path, hash)| document["files"][path.as_str()] == serde_json::Value::String(hash.clone())));
    assert_eq!(manifest.default_kws_group, default.group_id);
}

