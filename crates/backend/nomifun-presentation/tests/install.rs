//! Install state machine against local git fixtures and a fake runtime: no
//! network, no Python. `#[ignore]` tests at the bottom hit AtomGit, GitHub /
//! npmmirror and PyPI with the real runtime; run them by hand with `--ignored`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use git2::{Oid, Repository, Signature};
use nomifun_api_types::{
    PresentationCapabilityStatus, PresentationCapabilityView, PresentationInstallStage, WebSocketMessage,
};
use nomifun_presentation::{
    PrepareContext, PreparedRuntime, PresentationCapabilityService, RuntimePreparer, SourceSpec, UninstallError,
    manifest,
};
use nomifun_realtime::EventBroadcaster;
use tokio::sync::Notify;

const VERSION: &str = "6.8.0";

// ------------------------------------------------------------------ fixtures

struct Fixture {
    _temp: tempfile::TempDir,
    repo_dir: PathBuf,
    data_dir: PathBuf,
    commit: String,
}

fn fixture(skill_version: &str, with_symlink: bool) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let repo_dir = temp.path().join("origin");
    let data_dir = temp.path().join("data");
    let commit = build_repo(&repo_dir, skill_version, with_symlink);
    Fixture { _temp: temp, repo_dir, data_dir, commit }
}

/// `skills/ppt-master/{SKILL.md,scripts/attribution_guard.py}` plus an
/// unrelated `docs/README.md` that must not be materialized.
fn build_repo(dir: &Path, skill_version: &str, with_symlink: bool) -> String {
    let repo = Repository::init(dir).unwrap();
    let blob = |content: &str| repo.blob(content.as_bytes()).unwrap();

    let mut scripts = repo.treebuilder(None).unwrap();
    scripts.insert("attribution_guard.py", blob("print('ok')\n"), 0o100644).unwrap();
    let scripts = scripts.write().unwrap();

    let mut skill = repo.treebuilder(None).unwrap();
    let manifest = format!("---\nname: ppt-master\nmetadata:\n  version: \"{skill_version}\"\n---\n# PPT Master\n");
    skill.insert("SKILL.md", blob(&manifest), 0o100644).unwrap();
    skill.insert("scripts", scripts, 0o040000).unwrap();
    if with_symlink {
        skill.insert("evil", blob("/etc/passwd"), 0o120000).unwrap();
    }
    let skill = skill.write().unwrap();

    let mut skills = repo.treebuilder(None).unwrap();
    skills.insert("ppt-master", skill, 0o040000).unwrap();
    let skills = skills.write().unwrap();

    let mut docs = repo.treebuilder(None).unwrap();
    docs.insert("README.md", blob("not part of the skill\n"), 0o100644).unwrap();
    let docs = docs.write().unwrap();

    let mut root = repo.treebuilder(None).unwrap();
    root.insert("skills", skills, 0o040000).unwrap();
    root.insert("docs", docs, 0o040000).unwrap();
    let root = repo.find_tree(root.write().unwrap()).unwrap();

    let sig = Signature::now("tester", "tester@example.com").unwrap();
    let oid: Oid = repo.commit(Some("HEAD"), &sig, &sig, "fixture", &root, &[]).unwrap();
    oid.to_string()
}

fn spec(urls: &[&Path], commit: &str) -> SourceSpec {
    SourceSpec {
        urls: urls.iter().map(|path| path.to_string_lossy().into_owned()).collect(),
        commit: commit.to_owned(),
        skill_subdir: manifest::SKILL_SUBDIR.to_owned(),
    }
}

// --------------------------------------------------------------- fake runtime

#[derive(Default)]
struct FakeRuntime {
    calls: AtomicUsize,
    gate: Option<Arc<Notify>>,
    fail_at: Option<PresentationInstallStage>,
}

#[async_trait]
impl RuntimePreparer for FakeRuntime {
    async fn prepare(&self, context: &PrepareContext<'_>) -> Result<PreparedRuntime, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (context.report)(PresentationInstallStage::PreparingPython);
        if let Some(gate) = &self.gate {
            // Wait for the test, but honour cancel like the real steps do.
            loop {
                if context.cancel.load(Ordering::SeqCst) {
                    return Err("install cancelled".into());
                }
                if tokio::time::timeout(Duration::from_millis(20), gate.notified()).await.is_ok() {
                    break;
                }
            }
        }
        for stage in [
            PresentationInstallStage::PreparingPython,
            PresentationInstallStage::InstallingDependencies,
            PresentationInstallStage::SelfTest,
        ] {
            (context.report)(stage);
            if self.fail_at == Some(stage) {
                return Err(format!("fake failure at {stage:?}"));
            }
        }
        let python = context.layout.python();
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, b"").unwrap();
        Ok(PreparedRuntime::default())
    }
}

#[derive(Default)]
struct RecordingEvents {
    events: Mutex<Vec<WebSocketMessage<serde_json::Value>>>,
}

impl EventBroadcaster for RecordingEvents {
    fn broadcast(&self, event: WebSocketMessage<serde_json::Value>) {
        self.events.lock().unwrap().push(event);
    }
}

impl RecordingEvents {
    fn statuses(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|event| {
                assert_eq!(event.name, "presentationCapability.status");
                event.data["status"].as_str().unwrap().to_owned()
            })
            .collect()
    }
}

fn service(data_dir: &Path, spec: SourceSpec, runtime: Arc<FakeRuntime>) -> Arc<PresentationCapabilityService> {
    PresentationCapabilityService::with_parts(data_dir, spec, VERSION, runtime)
}

async fn wait_settled(service: &Arc<PresentationCapabilityService>) -> PresentationCapabilityView {
    for _ in 0..500 {
        let view = service.status();
        if view.status != PresentationCapabilityStatus::Installing {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("install did not settle: {:?}", service.status());
}

fn snapshots(data_dir: &Path) -> Vec<String> {
    let dir = data_dir.join("capabilities/presentation/ppt-master/snapshots");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// ---------------------------------------------------------------------- tests

#[tokio::test(flavor = "multi_thread")]
async fn fresh_install_materializes_only_the_skill_subtree() {
    let fx = fixture(VERSION, false);
    let runtime = Arc::new(FakeRuntime::default());
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime.clone());
    assert_eq!(svc.status().status, PresentationCapabilityStatus::NotInstalled);

    let started = svc.start_install(false);
    assert!(started.accepted);
    assert_eq!(started.capability.status, PresentationCapabilityStatus::Installing);

    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Ready, "{view:?}");
    assert_eq!(view.source_commit, fx.commit);
    assert!(view.failure.is_none());
    let skill_dir = PathBuf::from(view.skill_dir.unwrap());
    assert!(skill_dir.join("SKILL.md").is_file());
    assert!(skill_dir.join("scripts/attribution_guard.py").is_file());
    assert!(!skill_dir.join("docs").exists());
    assert!(PathBuf::from(view.python.unwrap()).is_file());
    // Temporary bare repository is gone; only the one snapshot remains.
    assert!(!skill_dir.parent().unwrap().join(".fetch").exists());
    assert_eq!(snapshots(&fx.data_dir).len(), 1);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_is_idempotent_and_force_keeps_one_rollback_snapshot() {
    let fx = fixture(VERSION, false);
    let runtime = Arc::new(FakeRuntime::default());
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime.clone());

    svc.start_install(false);
    wait_settled(&svc).await;
    let first = snapshots(&fx.data_dir);

    let again = svc.start_install(false);
    assert!(!again.accepted, "Ready must not reinstall without force");
    assert_eq!(again.capability.status, PresentationCapabilityStatus::Ready);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);

    assert!(svc.start_install(true).accepted);
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
    let second = snapshots(&fx.data_dir);
    assert_eq!(second.len(), 2, "current + previous: {second:?}");
    assert!(second.contains(&first[0]));

    assert!(svc.start_install(true).accepted);
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
    let third = snapshots(&fx.data_dir);
    assert_eq!(third.len(), 2, "oldest snapshot collected: {third:?}");
    assert!(!third.contains(&first[0]));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_install_requests_admit_one() {
    let fx = fixture(VERSION, false);
    let gate = Arc::new(Notify::new());
    let runtime = Arc::new(FakeRuntime { gate: Some(gate.clone()), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime.clone());

    assert!(svc.start_install(false).accepted);
    let second = svc.start_install(true);
    assert!(!second.accepted);
    assert_eq!(second.capability.status, PresentationCapabilityStatus::Installing);

    gate.notify_one();
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_commit_fails_cleanly() {
    let fx = fixture(VERSION, false);
    let missing = "0123456789abcdef0123456789abcdef01234567";
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], missing), Arc::new(FakeRuntime::default()));

    svc.start_install(false);
    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Failed);
    let failure = view.failure.unwrap();
    assert_eq!(failure.stage, Some(PresentationInstallStage::FetchingSkill));
    assert!(failure.message.contains("all ppt-master sources failed"), "{}", failure.message);
    assert!(snapshots(&fx.data_dir).is_empty(), "half-built snapshot left behind");
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_first_source_falls_back_to_the_next() {
    let fx = fixture(VERSION, false);
    let dead = fx.data_dir.parent().unwrap().join("no-such-repo");
    let svc = service(&fx.data_dir, spec(&[&dead, &fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));

    svc.start_install(false);
    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Ready, "{view:?}");
    assert_eq!(view.installed_from.as_deref(), Some(fx.repo_dir.to_string_lossy().as_ref()));
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_reinstall_keeps_the_working_snapshot() {
    let fx = fixture(VERSION, false);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    let ready = wait_settled(&svc).await;

    let failing = Arc::new(FakeRuntime { fail_at: Some(PresentationInstallStage::InstallingDependencies), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), failing);
    assert!(svc.start_install(true).accepted);
    let view = wait_settled(&svc).await;

    assert_eq!(view.status, PresentationCapabilityStatus::Ready, "old snapshot must stay usable");
    assert_eq!(view.skill_dir, ready.skill_dir);
    let failure = view.failure.expect("the failed attempt is still reported");
    assert_eq!(failure.stage, Some(PresentationInstallStage::InstallingDependencies));
    assert_eq!(snapshots(&fx.data_dir).len(), 1, "failed snapshot removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn symlink_in_skill_tree_is_rejected() {
    let fx = fixture(VERSION, true);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Failed);
    assert!(view.failure.unwrap().message.contains("unsupported entry evil"));
    assert!(snapshots(&fx.data_dir).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn skill_version_mismatch_is_rejected() {
    let fx = fixture("6.9.0", false);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Failed);
    assert!(view.failure.unwrap().message.contains("SKILL.md is not ppt-master 6.8.0"));
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_snapshot_reports_failed_and_force_repairs_it() {
    let fx = fixture(VERSION, false);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    let ready = wait_settled(&svc).await;
    std::fs::remove_file(ready.python.unwrap()).unwrap();

    let broken = svc.status();
    assert_eq!(broken.status, PresentationCapabilityStatus::Failed);
    let failure = broken.failure.unwrap();
    assert_eq!(failure.stage, None);
    assert!(failure.message.contains("Python interpreter"));

    assert!(svc.start_install(false).accepted, "Failed is not Ready, so no force needed");
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_the_pin_reports_not_installed() {
    let fx = fixture(VERSION, false);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);

    let other = "fedcba9876543210fedcba9876543210fedcba98";
    let upgraded = service(&fx.data_dir, spec(&[&fx.repo_dir], other), Arc::new(FakeRuntime::default()));
    assert_eq!(upgraded.status().status, PresentationCapabilityStatus::NotInstalled);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_before_any_install_settles_on_not_installed() {
    let fx = fixture(VERSION, false);
    let gate = Arc::new(Notify::new());
    let runtime = Arc::new(FakeRuntime { gate: Some(gate), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime);

    assert!(!svc.cancel().cancelled, "nothing to cancel yet");
    assert!(svc.start_install(false).accepted);
    let cancelled = svc.cancel();
    assert!(cancelled.cancelled);

    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::NotInstalled, "{view:?}");
    assert!(view.failure.is_none(), "a cancel is not a failure");
    assert!(snapshots(&fx.data_dir).is_empty(), "half-built snapshot removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_reinstall_keeps_the_working_snapshot_without_failure() {
    let fx = fixture(VERSION, false);
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    let ready = wait_settled(&svc).await;

    let gate = Arc::new(Notify::new());
    let gated = Arc::new(FakeRuntime { gate: Some(gate), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), gated);
    assert!(svc.start_install(true).accepted);
    assert!(svc.cancel().cancelled);
    let view = wait_settled(&svc).await;
    assert_eq!(view.status, PresentationCapabilityStatus::Ready);
    assert_eq!(view.skill_dir, ready.skill_dir);
    assert!(view.failure.is_none());
    assert_eq!(snapshots(&fx.data_dir).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_removes_everything_and_is_refused_while_installing() {
    let fx = fixture(VERSION, false);
    let gate = Arc::new(Notify::new());
    let runtime = Arc::new(FakeRuntime { gate: Some(gate.clone()), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime);

    assert_eq!(svc.uninstall().unwrap().status, PresentationCapabilityStatus::NotInstalled, "no-op when absent");
    assert!(svc.start_install(false).accepted);
    assert_eq!(svc.uninstall().unwrap_err(), UninstallError::Busy);
    gate.notify_one();
    let ready = wait_settled(&svc).await;
    assert_eq!(ready.status, PresentationCapabilityStatus::Ready);

    let view = svc.uninstall().unwrap();
    assert_eq!(view.status, PresentationCapabilityStatus::NotInstalled);
    assert!(view.failure.is_none());
    assert_eq!(svc.status().status, PresentationCapabilityStatus::NotInstalled);
    assert!(snapshots(&fx.data_dir).is_empty());
    let skill_dir = PathBuf::from(ready.skill_dir.unwrap());
    for _ in 0..100 {
        if !skill_dir.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!skill_dir.exists(), "background delete finished");

    // A fresh install works after an uninstall.
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
    svc.start_install(false);
    assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
}

#[tokio::test(flavor = "multi_thread")]
async fn events_are_published_in_order_and_end_on_the_settled_state() {
    let fx = fixture(VERSION, false);
    let gate = Arc::new(Notify::new());
    let runtime = Arc::new(FakeRuntime { gate: Some(gate.clone()), ..Default::default() });
    let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime);
    let events = Arc::new(RecordingEvents::default());
    svc.set_event_broadcaster(events.clone());

    assert!(svc.start_install(false).accepted);
    // Hold the install long enough for at least one progress tick.
    tokio::time::sleep(Duration::from_millis(700)).await;
    gate.notify_one();
    wait_settled(&svc).await;

    let statuses = events.statuses();
    assert_eq!(statuses.first().map(String::as_str), Some("installing"), "{statuses:?}");
    assert_eq!(statuses.last().map(String::as_str), Some("ready"), "{statuses:?}");
    assert!(statuses[..statuses.len() - 1].iter().all(|status| status == "installing"), "{statuses:?}");
    let first = events.events.lock().unwrap()[0].data.clone();
    assert!(first["stage"].is_string(), "installing events carry the stage");

    svc.uninstall().unwrap();
    assert_eq!(events.statuses().last().map(String::as_str), Some("not_installed"));
}

// --------------------------------------------------------------------- routes

mod routes {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use nomifun_presentation::{PresentationRouterState, presentation_routes};
    use tower::ServiceExt;

    fn app(svc: Arc<PresentationCapabilityService>) -> axum::Router {
        presentation_routes(PresentationRouterState { service: svc })
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: &'static str) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn malformed_install_body_is_400_and_starts_nothing() {
        let fx = fixture(VERSION, false);
        let runtime = Arc::new(FakeRuntime::default());
        let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime.clone());
        let (status, body) = call(app(svc.clone()), "POST", "/api/presentation/capability/install", "{not json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "PRESENTATION_INSTALL_BAD_REQUEST");
        let (status, _) =
            call(app(svc.clone()), "POST", "/api/presentation/capability/install", r#"{"url":"https://evil"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "unknown fields are rejected");
        assert_eq!(svc.status().status, PresentationCapabilityStatus::NotInstalled);
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_install_body_starts_an_install_and_status_reports_it() {
        let fx = fixture(VERSION, false);
        let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
        let (status, body) = call(app(svc.clone()), "POST", "/api/presentation/capability/install", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["accepted"], true);

        wait_settled(&svc).await;
        let (status, body) = call(app(svc), "GET", "/api/presentation/capability/status", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["status"], "ready");
        assert_eq!(body["data"]["capability_id"], "presentation.ppt-master");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_and_delete_routes_follow_the_service() {
        let fx = fixture(VERSION, false);
        let gate = Arc::new(Notify::new());
        let runtime = Arc::new(FakeRuntime { gate: Some(gate.clone()), ..Default::default() });
        let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), runtime);

        let (status, body) = call(app(svc.clone()), "POST", "/api/presentation/capability/cancel", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["cancelled"], false);

        svc.start_install(false);
        let (status, body) = call(app(svc.clone()), "DELETE", "/api/presentation/capability", "").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "PRESENTATION_CAPABILITY_BUSY");
        gate.notify_one();
        assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);

        let (status, body) = call(app(svc.clone()), "DELETE", "/api/presentation/capability", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["capability"]["status"], "not_installed");
    }
}

// ------------------------------------------------------------ python download

mod python_download {
    use super::*;
    use axum::Router;
    use axum::routing::get;
    use nomifun_presentation::{InstallProgress, PythonSource, python_dist};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::AtomicBool;

    /// Minimal archive in the python-build-standalone layout.
    fn archive() -> Vec<u8> {
        let exe = if cfg!(windows) { "python/python.exe" } else { "python/bin/python3" };
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in [(exe, &b"#!fake"[..]), ("python/lib/os.py", &b"pass"[..])] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, path, data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// `/good` serves the archive, `/bad` serves the same length of zeros,
    /// `/missing` is a 404. Returns the base URL and a hit counter.
    async fn server(body: Vec<u8>) -> (String, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let zeros = vec![0_u8; body.len()];
        let counter = hits.clone();
        let app = Router::new()
            .route(
                "/good",
                get(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let body = body.clone();
                    async move { body }
                }),
            )
            .route("/bad", get(move || {
                let zeros = zeros.clone();
                async move { zeros }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, hits)
    }

    fn source(urls: Vec<String>, body: &[u8]) -> PythonSource {
        PythonSource {
            id: "3.12.16-test".into(),
            urls,
            sha256: format!("{:x}", Sha256::digest(body)),
            size: body.len() as u64,
            archive_root: "python".into(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn verified_archive_is_published_once_and_reused() {
        let body = archive();
        let (base, hits) = server(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let src = source(vec![format!("{base}/missing"), format!("{base}/bad"), format!("{base}/good")], &body);
        let progress = InstallProgress::default();
        let cancel = AtomicBool::new(false);

        let home = python_dist::ensure(dir.path(), &src, &progress, &cancel).await.unwrap();
        assert!(python_dist::is_ready(&home));
        assert_eq!(std::fs::read(home.join("lib/os.py")).unwrap(), b"pass");
        assert_eq!(progress.received_bytes.load(Ordering::Relaxed), body.len() as u64);
        assert_eq!(progress.total_bytes.load(Ordering::Relaxed), body.len() as u64);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["3.12.16-test".to_owned()], "no partial downloads left");

        python_dist::ensure(dir.path(), &src, &progress, &cancel).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1, "ready home is reused");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn digest_mismatch_on_every_source_fails_without_publishing() {
        let body = archive();
        let (base, _) = server(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let src = source(vec![format!("{base}/bad")], &body);
        let error = python_dist::ensure(dir.path(), &src, &InstallProgress::default(), &AtomicBool::new(false))
            .await
            .unwrap_err();
        assert!(error.contains("SHA-256 mismatch"), "{error}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing published or left behind");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_stops_before_downloading() {
        let body = archive();
        let (base, hits) = server(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let src = source(vec![format!("{base}/good")], &body);
        let error = python_dist::ensure(dir.path(), &src, &InstallProgress::default(), &AtomicBool::new(true))
            .await
            .unwrap_err();
        assert_eq!(error, "install cancelled");
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }
}

// --------------------------------------------------------------- run_script

mod run_script {
    use super::*;
    use nomifun_presentation::ScriptError;

    async fn ready_service(fx: &Fixture) -> Arc<PresentationCapabilityService> {
        let svc = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
        svc.start_install(false);
        assert_eq!(wait_settled(&svc).await.status, PresentationCapabilityStatus::Ready);
        svc
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refused_until_ready_and_for_bad_requests() {
        let fx = fixture(VERSION, false);
        let workspace = fx.data_dir.parent().unwrap().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let fresh = service(&fx.data_dir, spec(&[&fx.repo_dir], &fx.commit), Arc::new(FakeRuntime::default()));
        let error = fresh.run_script("attribution_guard.py", &[], &workspace, None).await.unwrap_err();
        assert!(matches!(error, ScriptError::NotReady(_)), "{error:?}");
        assert!(error.to_string().contains("nomi_presentation_install"));

        let svc = ready_service(&fx).await;
        for (script, cwd) in [
            ("../SKILL.md", None),
            ("missing.py", None),
            ("confirm_ui/server.py", None),
            ("attribution_guard.py", Some("..")),
        ] {
            let error = svc.run_script(script, &[], &workspace, cwd).await.unwrap_err();
            assert!(matches!(error, ScriptError::Invalid(_)), "{script} {cwd:?}: {error:?}");
        }
    }

    /// Success path with a real interpreter: a venv built from the system
    /// Python stands in for the private one. Skipped when none is on PATH.
    #[tokio::test(flavor = "multi_thread")]
    async fn runs_the_script_in_the_workspace_with_the_snapshot_interpreter() {
        let Some(system_python) = ["python3", "python"]
            .iter()
            .find(|name| {
                std::process::Command::new(name)
                    .args(["-c", "import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)"])
                    .output()
                    .is_ok_and(|out| out.status.success())
            })
            .copied()
        else {
            eprintln!("skipped: no system Python");
            return;
        };
        let fx = fixture(VERSION, false);
        let svc = ready_service(&fx).await;
        let view = svc.status();
        let skill_dir = PathBuf::from(view.skill_dir.unwrap());
        let venv = PathBuf::from(view.python.unwrap()).parent().unwrap().parent().unwrap().to_path_buf();
        std::fs::remove_dir_all(&venv).unwrap();
        let made = std::process::Command::new(system_python).args(["-m", "venv"]).arg(&venv).status().unwrap();
        assert!(made.success());
        std::fs::write(
            skill_dir.join("scripts/echo_cwd.py"),
            "import os, sys\nprint('cwd=' + os.getcwd())\nprint('args=' + '|'.join(sys.argv[1:]))\nsys.exit(3)\n",
        )
        .unwrap();
        let workspace = fx.data_dir.parent().unwrap().join("ws");
        std::fs::create_dir_all(workspace.join("projects")).unwrap();

        let run = svc
            .run_script("scripts/echo_cwd.py", &["a b".into(), "中文".into()], &workspace, Some("projects"))
            .await
            .unwrap();
        assert_eq!(run.exit_code, Some(3), "{run:?}");
        assert!(!run.timed_out);
        assert_eq!(run.script, "echo_cwd.py");
        let expected_cwd = dunce::canonicalize(workspace.join("projects")).unwrap();
        assert!(run.output.contains(&format!("cwd={}", expected_cwd.display())), "{}", run.output);
        assert!(run.output.contains("args=a b|中文"), "{}", run.output);
    }
}

// ------------------------------------------------------------- manual (network)

/// Real pinned portable Python for this host: GitHub Release, then npmmirror.
/// `cargo test -p nomifun-presentation --test install -- --ignored pinned_python`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads ~22-35 MB from GitHub / npmmirror"]
async fn pinned_python_downloads_verifies_and_runs() {
    let temp = tempfile::tempdir().unwrap();
    let source = nomifun_presentation::PythonSource::pinned().expect("supported host");
    let progress = nomifun_presentation::InstallProgress::default();
    let home = nomifun_presentation::python_dist::ensure(
        temp.path(),
        &source,
        &progress,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await
    .expect("pinned Python");
    let output = std::process::Command::new(nomifun_presentation::python_dist::interpreter(&home))
        .args(["-c", manifest::PYTHON_PROBE])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3.12");
    eprintln!("{} ({} bytes)", home.display(), progress.received_bytes.load(Ordering::Relaxed));
}

/// Real shallow fetch-by-commit from the pinned remotes through git2/https.
/// `cargo test -p nomifun-presentation --test install -- --ignored pinned_remote`
#[test]
#[ignore = "network: fetches ~53 MB from AtomGit/GitHub"]
fn pinned_remote_fetch_materializes_the_real_skill() {
    let temp = tempfile::tempdir().unwrap();
    let dest = temp.path().join("skill");
    let progress = nomifun_presentation::InstallProgress::default();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let fetched = nomifun_presentation::source::fetch_skill(
        &SourceSpec::pinned(),
        &temp.path().join("work"),
        &dest,
        &progress,
        &cancel,
    )
    .expect("pinned fetch");
    assert_eq!(fetched.commit, manifest::SOURCE_COMMIT);
    assert!(dest.join("SKILL.md").is_file());
    assert!(dest.join(manifest::ATTRIBUTION_GUARD).is_file());
    assert!(dest.join("templates/icons/tabler-outline").is_dir());
    assert!(progress.received_bytes.load(Ordering::Relaxed) > 1_000_000);
    eprintln!("fetched {} from {} ({} bytes)", fetched.tree, fetched.url, progress.received_bytes.load(Ordering::Relaxed));
}

/// Full production install: AtomGit + portable Python + locked wheels +
/// self-test. `cargo test -p nomifun-presentation --test install -- --ignored real_install`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads ppt-master, Python and wheels (~120 MB)"]
async fn real_install_reaches_ready() {
    let temp = tempfile::tempdir().unwrap();
    let svc = PresentationCapabilityService::new(temp.path());
    assert!(svc.start_install(false).accepted);
    let mut view = svc.status();
    for _ in 0..(30 * 60) {
        view = svc.status();
        if view.status != PresentationCapabilityStatus::Installing {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert_eq!(view.status, PresentationCapabilityStatus::Ready, "{view:#?}");
    let python = view.python.clone().unwrap();
    assert!(python.contains("venv"), "{python}");
    eprintln!("{view:#?}");
}
