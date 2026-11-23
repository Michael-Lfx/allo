//! TC-17 exercises the application's real native save-picker receiver with
//! immutable CSV bytes created by ArtifactStore. TestPlatform supplies the
//! user's selection; it never reads or writes the suggested Documents folder.

use super::{FlowyApp, Screen, Section, state::ActiveDialog};
use crate::session::SessionStore;
use flowy_agent::{
    ArtifactStore, DeliveryCommit, PrepareArtifact, SessionOrigin,
};
use flowy_studio::library::LibraryEvent;
use gpui::{Entity, TestAppContext, prelude::*};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const CSV: &[u8] = b"name,total\nalpha,7\nbeta,11\n";
const LEGACY_CATALOG: &[u8] = b"{broken legacy library catalog\n";

struct Fixture {
    root: PathBuf,
    store: ArtifactStore,
    artifact_id: String,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build-test");
        fs::create_dir_all(&base).unwrap();
        let root = loop {
            let candidate = base.join(format!(
                "artifact-save-picker-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    panic!("could not create isolated picker fixture: {error}")
                }
            }
        };
        let output = root.join("turn-output");
        fs::create_dir(&output).unwrap();
        fs::create_dir(root.join("exports")).unwrap();
        fs::write(output.join("report.csv"), CSV).unwrap();
        fs::write(root.join("library.json"), LEGACY_CATALOG).unwrap();
        let store = ArtifactStore::open(root.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "report.csv".into(),
                source_relative_path: "report.csv".into(),
                conversation_import: false,
                session_id: "picker-session".into(),
                submission_id: "picker-submission".into(),
                tool_call_id: "picker-call".into(),
                delivery_key: "csv-report".into(),
                display_name: Some("销售汇总.csv".into()),
                origin: SessionOrigin::Home,
            })
            .unwrap();
        // The fixture supplies the successful durable-history proof. Bytes,
        // receipt, manifests and independent marker use production Store code.
        store
            .recover_commits(&[DeliveryCommit {
                session_id: receipt.session_id.clone(),
                submission_id: receipt.submission_id.clone(),
                artifact_ids: vec![receipt.artifact_id.clone()],
                completed_successfully: true,
                committed_at_ms: receipt.created_at_ms,
                origin: SessionOrigin::Home,
                origin_title_snapshot: "CSV picker fixture".into(),
            }])
            .unwrap();
        Self {
            root,
            store,
            artifact_id: receipt.artifact_id,
        }
    }

    fn assert_sources_unchanged(&self) {
        assert_eq!(self.store.read_payload(&self.artifact_id).unwrap(), CSV);
        assert_eq!(
            fs::read(self.root.join("turn-output/report.csv")).unwrap(),
            CSV
        );
        assert_eq!(
            fs::read(self.root.join("library.json")).unwrap(),
            LEGACY_CATALOG,
            "the production library must not load, repair, or rewrite the legacy local-import catalog"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn app_with_loaded_artifact(
    cx: &TestAppContext,
    fixture: &Fixture,
) -> Entity<FlowyApp> {
    let app = cx.update(|cx| {
        cx.new(|cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(fixture.root.clone()),
                false,
                cx,
            );
            app.screen = Screen::Main;
            app.section = Section::Library;
            app.active_dialog = ActiveDialog::None;
            app.ota.checked = true;
            app
        })
    });
    // FlowyApp owns the asynchronous library query; no test-only row is
    // injected into LibraryPanel or the conversation projection.
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        let items = &app.library.read(cx).model.items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, fixture.artifact_id);
        assert_eq!(items[0].title, "销售汇总.csv");
    });
    app
}

fn request_save(
    cx: &TestAppContext,
    app: &Entity<FlowyApp>,
    fixture: &Fixture,
) {
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.library_event(
                &LibraryEvent::SaveAsRequested(fixture.artifact_id.clone()),
                cx,
            );
        })
    });
    assert!(
        cx.did_prompt_for_new_path(),
        "the native save-path prompt must actually be requested"
    );
}

fn assert_export_failure(cx: &TestAppContext, app: &Entity<FlowyApp>) {
    cx.update(|cx| {
        assert_eq!(app.read(cx).notice, Some("library.error.export"));
    });
}

#[gpui::test]
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "GPUI test harness passes &mut TestAppContext"
)]
fn save_picker_cancellation_creates_no_export(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let app = app_with_loaded_artifact(cx, &fixture);
    request_save(cx, &app, &fixture);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
    assert!(
        fs::read_dir(fixture.root.join("exports"))
            .unwrap()
            .next()
            .is_none()
    );
    cx.update(|cx| {
        assert_ne!(app.read(cx).notice, Some("library.error.export"))
    });
    fixture.assert_sources_unchanged();
    drop(app);
    cx.run_until_parked();
}

#[gpui::test]
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "GPUI test harness passes &mut TestAppContext"
)]
fn save_picker_exports_snapshot_and_replaces_an_accepted_existing_path(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let app = app_with_loaded_artifact(cx, &fixture);
    let selected = fixture.root.join("exports/exported.csv");
    request_save(cx, &app, &fixture);
    cx.simulate_new_path_selection(|_| Some(selected.clone()));
    cx.run_until_parked();
    assert_eq!(fs::read(&selected).unwrap(), CSV);
    fixture.assert_sources_unchanged();

    fs::write(
        &selected,
        b"existing user file before confirmed replacement",
    )
    .unwrap();
    request_save(cx, &app, &fixture);
    // Returning this existing path models a platform save dialog that accepted
    // its overwrite confirmation. This proves the host copy/replace behavior;
    // it does not substitute for visually checking each OS's native prompt.
    cx.simulate_new_path_selection(|_| Some(selected.clone()));
    cx.run_until_parked();
    assert_eq!(fs::read(&selected).unwrap(), CSV);
    assert_eq!(
        fs::read_dir(fixture.root.join("exports")).unwrap().count(),
        1
    );
    fixture.assert_sources_unchanged();
    drop(app);
    cx.run_until_parked();
}

#[gpui::test]
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "GPUI test harness passes &mut TestAppContext"
)]
fn save_picker_rejects_a_directory_destination_and_preserves_existing_data(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let app = app_with_loaded_artifact(cx, &fixture);
    let selected = fixture.root.join("exports/existing-directory.csv");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("keep.txt"), b"keep directory contents").unwrap();
    let old_export = fixture.root.join("exports/previous.csv");
    fs::write(&old_export, b"preserve previous export").unwrap();
    request_save(cx, &app, &fixture);
    cx.simulate_new_path_selection(|_| Some(selected.clone()));
    cx.run_until_parked();
    assert_export_failure(cx, &app);
    assert!(selected.is_dir());
    assert_eq!(
        fs::read(selected.join("keep.txt")).unwrap(),
        b"keep directory contents"
    );
    assert_eq!(fs::read(old_export).unwrap(), b"preserve previous export");
    assert_eq!(
        fs::read_dir(fixture.root.join("exports")).unwrap().count(),
        2,
        "a failed atomic copy must not leave its temporary export beside the destination"
    );
    fixture.assert_sources_unchanged();
    drop(app);
    cx.run_until_parked();
}

#[cfg(windows)]
#[gpui::test]
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "GPUI test harness passes &mut TestAppContext"
)]
fn save_picker_reports_a_locked_destination_without_truncating_it(
    cx: &mut TestAppContext,
) {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let app = app_with_loaded_artifact(cx, &fixture);
    let selected = fixture.root.join("exports/locked.csv");
    fs::write(&selected, b"keep locked destination").unwrap();
    let occupied = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&selected)
        .unwrap();
    request_save(cx, &app, &fixture);
    cx.simulate_new_path_selection(|_| Some(selected.clone()));
    cx.run_until_parked();
    assert_export_failure(cx, &app);
    assert_eq!(fs::read(&selected).unwrap(), b"keep locked destination");
    assert_eq!(
        fs::read_dir(fixture.root.join("exports")).unwrap().count(),
        1
    );
    fixture.assert_sources_unchanged();
    drop(occupied);
    drop(app);
    cx.run_until_parked();
}
