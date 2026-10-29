//! Isolated end-to-end install chain: loopback mirror -> .part -> SHA -> Ready.
//! It uses synthetic bytes only; the real 246MB suite is verified separately.

use std::collections::HashMap;
use std::path::PathBuf;

use nomifun_voice_models::downloader::MirrorConfig;
use nomifun_voice_models::verifier::{DOWNLOAD_FILES, compute_sha256, default_canonical_manifest, parse_checksums_file};
use nomifun_voice_models::{EXPECTED_FILES, ModelManager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn build_source(root: &std::path::Path) -> HashMap<String, String> {
    let mut manifest = HashMap::new();
    for &relative in EXPECTED_FILES {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = if DOWNLOAD_FILES.contains(&relative) {
            format!("isolated-install-content-{relative}").into_bytes()
        } else {
            b"isolated-install-metadata".to_vec()
        };
        std::fs::write(&path, &content).unwrap();
        if DOWNLOAD_FILES.contains(&relative) {
            manifest.insert(relative.to_owned(), compute_sha256(&content));
        }
    }
    let checksums = manifest
        .iter()
        .map(|(path, hash)| format!("{hash}  {path}\n"))
        .collect::<String>();
    std::fs::write(root.join("checksums.sha256"), checksums).unwrap();
    manifest
}

async fn serve_source(
    root: PathBuf,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break; };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buffer.windows(4).any(|part| part == b"\r\n\r\n") {
                let Ok(read) = socket.read(&mut chunk).await else { return; };
                if read == 0 { break; }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let request = String::from_utf8_lossy(&buffer);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .trim_start_matches('/');
            let body = std::fs::read(root.join(path)).unwrap_or_default();
            let status = if body.is_empty() { "404 Not Found" } else { "200 OK" };
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        }
    });
    (base, server)
}

#[tokio::test]
async fn isolated_install_reaches_ready_only_after_sha() {
    let source = tempfile::tempdir().unwrap();
    let manifest = build_source(source.path());
    let checksums = std::fs::read_to_string(source.path().join("checksums.sha256")).unwrap();
    assert!(
        !parse_checksums_file(&checksums).is_empty(),
        "fixture checksums must cover the downloaded files"
    );
    let (base, server) = serve_source(source.path().to_path_buf()).await;
    let target = tempfile::tempdir().unwrap();
    let mirror = MirrorConfig {
        id: "isolated-loopback".into(),
        base_url: base,
        probe_url: None,
        is_modelscope: false,
        is_fallback_cdn: false,
    };
    let manager = ModelManager::with_config(target.path(), vec![mirror], manifest);
    assert!(manager.install(None).await.unwrap().accepted);

    let mut state = manager.get_status().await.state;
    for _ in 0..200 {
        if !state.is_busy() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        state = manager.get_status().await.state;
    }
    assert!(
        state.is_ready(),
        "the isolated install chain must reach Ready, got {state:?}"
    );
    for &relative in EXPECTED_FILES {
        assert!(
            target.path().join(relative).is_file(),
            "installed suite is missing {relative}"
        );
    }
    // The manifest used by the production verifier is still the canonical one.
    assert_eq!(
        default_canonical_manifest().len(),
        DOWNLOAD_FILES.len(),
        "the production manifest must cover every downloaded asset"
    );
    server.abort();
}
