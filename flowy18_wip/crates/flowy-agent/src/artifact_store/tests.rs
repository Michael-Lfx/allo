/// Empty slice with an explicit element type: a bare `[]` cannot be
/// inferred against `[String]` in these assertions.
const NO_DIAGNOSTICS: [String; 0] = [];

use super::{
    ArtifactStore, MAX_RETAINED_VIEW_COPIES, PREPARED_RETENTION_MS,
    PrepareArtifact, PublicationState, VIEW_COPY_RETENTION_MS, ViewCopyMeta,
};
use flowy_agent_core::{DeliveryCommit, SessionOrigin};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("flowy-artifacts-{stamp}"));
        fs::create_dir_all(path.join("workspace/outputs")).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(root: &Path, relative_path: &str, key: &str) -> PrepareArtifact {
    PrepareArtifact {
        source_root: root.join("workspace"),
        relative_path: relative_path.into(),
        source_relative_path: relative_path.into(),
        conversation_import: false,
        session_id: "session-1".into(),
        submission_id: "submission-1".into(),
        tool_call_id: format!("call-{key}"),
        delivery_key: key.into(),
        display_name: None,
        origin: SessionOrigin::Home,
    }
}
fn store(root: &Path) -> ArtifactStore {
    fs::create_dir_all(root.join("workspace/outputs")).unwrap();
    ArtifactStore::open(root.join("artifacts")).unwrap()
}
fn commit(receipt: &flowy_agent_core::ArtifactReceipt) -> DeliveryCommit {
    DeliveryCommit {
        session_id: receipt.session_id.clone(),
        submission_id: receipt.submission_id.clone(),
        artifact_ids: vec![receipt.artifact_id.clone()],
        completed_successfully: true,
        origin: SessionOrigin::Home,
        origin_title_snapshot: "Home task".into(),
        committed_at_ms: 42,
    }
}

#[test]
fn legacy_manifest_without_source_relative_path_deserializes_empty() {
    let manifest: super::ArtifactManifest =
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "receipt": {
                "artifact_id": "artifact-1",
                "delivery_key": "report",
                "session_id": "session-1",
                "submission_id": "submission-1",
                "tool_call_id": "call-1",
                "display_name": "report.md",
                "size_bytes": 1,
                "sha256": "digest",
                "created_at_ms": 1
            },
            "original_basename": "report.md",
            "origin": { "kind": "home" },
            "payload_relative_path": "items/artifact-1/payload",
            "publication_state": "committed"
        }))
        .unwrap();
    assert_eq!(manifest.source_relative_path, "");
    assert!(!manifest.conversation_import);
    assert!(!manifest.superseded);
}

fn create_sparse_file(path: &std::path::Path, length: u64) {
    fs::File::create(path).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // SetEndOfFile alone does not promise a sparse NTFS allocation.
        // Mark the test-owned file before changing its logical length so
        // quota tests never reserve 5 GiB of real disk blocks.
        let output = std::process::Command::new("fsutil.exe")
            .args(["sparse", "setflag"])
            .arg(path)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not mark test file sparse: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(length).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn single_file_limit_accepts_exact_bounded_read_and_rejects_one_byte_over_before_copy()
 {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let path = scratch.0.join("workspace/outputs/large.bin");
    create_sparse_file(&path, super::MAX_ARTIFACT_BYTES);
    // Exercise the actual bounded digest reader at exactly 128 MiB
    // without writing a 128 MiB snapshot into the managed store.
    let mut source = fs::File::open(&path).unwrap();
    let exact = super::snapshot_file(&mut source).unwrap();
    assert_eq!(exact.size, super::MAX_ARTIFACT_BYTES);
    assert_eq!(exact.prefix, vec![0; 16]);
    drop(source);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(super::MAX_ARTIFACT_BYTES + 1)
        .unwrap();
    let error = store
        .prepare(request(&scratch.0, "outputs/large.bin", "over-limit"))
        .unwrap_err();
    assert!(error.to_string().contains("128 MiB"));
    let usage = store.usage().unwrap();
    assert_eq!(usage.stored_items, 0);
    assert_eq!(usage.staging_bytes, 0);
    assert_eq!(store.scan_locked().unwrap().manifests.as_slice(), []);
}

#[test]
fn ninth_file_is_rejected_without_damaging_the_first_eight() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let mut prepared = Vec::new();
    for index in 0..super::MAX_DELIVERY_FILES {
        let file = format!("report-{index}.csv");
        let bytes =
            format!("index,value\n{index},{}\n", index + 10).into_bytes();
        fs::write(scratch.0.join("workspace/outputs").join(&file), &bytes)
            .unwrap();
        let receipt = store
            .prepare(request(
                &scratch.0,
                &format!("outputs/{file}"),
                &format!("slot-{index}"),
            ))
            .unwrap();
        prepared.push((receipt, bytes));
    }
    fs::write(
        scratch.0.join("workspace/outputs/ninth.csv"),
        b"index,value\n9,19\n",
    )
    .unwrap();
    let error = store
        .prepare(request(&scratch.0, "outputs/ninth.csv", "ninth"))
        .unwrap_err();
    assert!(error.to_string().contains("per-turn"));
    assert_eq!(
        store.usage().unwrap().stored_items,
        super::MAX_DELIVERY_FILES
    );
    assert_eq!(
        store.scan_locked().unwrap().manifests.len(),
        super::MAX_DELIVERY_FILES
    );
    assert!(
        store.list().unwrap().is_empty(),
        "prepared files are not delivered yet"
    );
    let mut proof = commit(&prepared[0].0);
    proof.artifact_ids = prepared
        .iter()
        .map(|(receipt, _)| receipt.artifact_id.clone())
        .collect();
    store.commit_delivery(&proof).unwrap();
    assert_eq!(store.list().unwrap().len(), super::MAX_DELIVERY_FILES);
    for (receipt, bytes) in prepared {
        assert_eq!(store.read_payload(&receipt.artifact_id).unwrap(), bytes);
    }
}

#[test]
fn superseded_conversation_import_releases_hidden_payload() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/notes.md");
    fs::write(&source, b"old").unwrap();
    let mut first_request =
        request(&scratch.0, "outputs/notes.md", "old-import");
    first_request.conversation_import = true;
    let first = store.prepare(first_request).unwrap();
    store.commit_delivery(&commit(&first)).unwrap();

    fs::write(&source, b"new").unwrap();
    let mut second_request =
        request(&scratch.0, "outputs/notes.md", "new-import");
    second_request.conversation_import = true;
    second_request.submission_id = "submission-2".into();
    let second = store.prepare(second_request).unwrap();
    store.commit_delivery(&commit(&second)).unwrap();

    let visible = store.list().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].manifest.receipt.artifact_id, second.artifact_id);
    let retired = store.get(&first.artifact_id).unwrap().unwrap();
    assert_eq!(
        retired.manifest.publication_state,
        PublicationState::Deleted
    );
    assert!(store.read_payload(&first.artifact_id).is_err());
    assert_eq!(store.usage().unwrap().stored_items, 1);
}

#[test]
fn returning_to_a_superseded_content_version_creates_a_new_snapshot() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/notes.md");

    fs::write(&source, b"version-a").unwrap();
    let mut first_request = request(&scratch.0, "outputs/notes.md", "first-a");
    first_request.conversation_import = true;
    let first = store.prepare(first_request).unwrap();
    store.commit_delivery(&commit(&first)).unwrap();

    fs::write(&source, b"version-b").unwrap();
    let mut second_request =
        request(&scratch.0, "outputs/notes.md", "version-b");
    second_request.conversation_import = true;
    second_request.submission_id = "submission-2".into();
    let second = store.prepare(second_request).unwrap();
    store.commit_delivery(&commit(&second)).unwrap();
    assert!(
        store
            .get(&first.artifact_id)
            .unwrap()
            .unwrap()
            .manifest
            .superseded
    );

    fs::write(&source, b"version-a").unwrap();
    let mut third_request = request(&scratch.0, "outputs/notes.md", "return-a");
    third_request.conversation_import = true;
    third_request.submission_id = "submission-3".into();
    let third = store.prepare(third_request).unwrap();
    assert_ne!(third.artifact_id, first.artifact_id);
    store.commit_delivery(&commit(&third)).unwrap();

    let visible = store.list().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].manifest.receipt.artifact_id, third.artifact_id);
    assert_eq!(
        store.read_payload(&third.artifact_id).unwrap(),
        b"version-a"
    );
    assert!(store.read_payload(&second.artifact_id).is_err());
}

#[test]
fn deleted_content_can_return_after_a_newer_source_version() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/notes.md");

    fs::write(&source, b"version-a").unwrap();
    let mut first_request = request(&scratch.0, "outputs/notes.md", "first-a");
    first_request.conversation_import = true;
    let first = store.prepare(first_request).unwrap();
    store.commit_delivery(&commit(&first)).unwrap();
    store.trash(&first.artifact_id).unwrap();
    store.permanently_delete(&first.artifact_id).unwrap();

    fs::write(&source, b"version-b").unwrap();
    let mut second_request =
        request(&scratch.0, "outputs/notes.md", "version-b");
    second_request.conversation_import = true;
    second_request.submission_id = "submission-2".into();
    let second = store.prepare(second_request).unwrap();
    store.commit_delivery(&commit(&second)).unwrap();

    fs::write(&source, b"version-a").unwrap();
    let mut third_request = request(&scratch.0, "outputs/notes.md", "return-a");
    third_request.conversation_import = true;
    third_request.submission_id = "submission-3".into();
    let third = store.prepare(third_request).unwrap();
    assert_ne!(third.artifact_id, first.artifact_id);
    store.commit_delivery(&commit(&third)).unwrap();

    let visible = store.list().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].manifest.receipt.artifact_id, third.artifact_id);
}

#[test]
fn superseded_explicit_delivery_is_retained_for_chat_cards() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/notes.md");
    fs::write(&source, b"explicit").unwrap();
    let first = store
        .prepare(request(&scratch.0, "outputs/notes.md", "explicit"))
        .unwrap();
    store.commit_delivery(&commit(&first)).unwrap();

    fs::write(&source, b"imported").unwrap();
    let mut imported_request =
        request(&scratch.0, "outputs/notes.md", "imported");
    imported_request.conversation_import = true;
    imported_request.submission_id = "submission-2".into();
    let imported = store.prepare(imported_request).unwrap();
    store.commit_delivery(&commit(&imported)).unwrap();

    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(store.read_payload(&first.artifact_id).unwrap(), b"explicit");
    assert_eq!(
        store.read_payload(&imported.artifact_id).unwrap(),
        b"imported"
    );
}

#[test]
fn same_named_files_in_other_sessions_keep_independent_versions() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/report.csv");
    fs::write(&source, b"version,value\n1,old\n").unwrap();
    let first = store
        .prepare(request(&scratch.0, "outputs/report.csv", "report"))
        .unwrap();
    store.commit_delivery(&commit(&first)).unwrap();
    fs::write(&source, b"version,value\n2,new\n").unwrap();
    let mut next_turn = request(&scratch.0, "outputs/report.csv", "report");
    next_turn.submission_id = "submission-2".into();
    let second = store.prepare(next_turn).unwrap();
    store.commit_delivery(&commit(&second)).unwrap();
    // Identical bytes in a different source session are independently owned.
    fs::write(&source, b"version,value\n1,old\n").unwrap();
    let mut next_session = request(&scratch.0, "outputs/report.csv", "report");
    next_session.session_id = "session-2".into();
    let third = store.prepare(next_session).unwrap();
    store.commit_delivery(&commit(&third)).unwrap();
    assert_eq!(
        [&first.artifact_id, &second.artifact_id, &third.artifact_id]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(first.display_name, "report.csv");
    assert_eq!(second.display_name, first.display_name);
    assert_eq!(third.display_name, first.display_name);
    assert_eq!(first.sha256, third.sha256);
    assert_ne!(first.sha256, second.sha256);
    fs::remove_file(source).unwrap();
    assert_eq!(
        store.read_payload(&first.artifact_id).unwrap(),
        b"version,value\n1,old\n"
    );
    assert_eq!(
        store.read_payload(&second.artifact_id).unwrap(),
        b"version,value\n2,new\n"
    );
    assert_eq!(
        store.read_payload(&third.artifact_id).unwrap(),
        b"version,value\n1,old\n"
    );
    let visible = store.list().unwrap();
    assert_eq!(visible.len(), 2);
    let visible_ids = visible
        .iter()
        .map(|record| record.manifest.receipt.artifact_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!visible_ids.contains(first.artifact_id.as_str()));
    assert!(visible_ids.contains(second.artifact_id.as_str()));
    assert!(visible_ids.contains(third.artifact_id.as_str()));
}

#[test]
fn sparse_raw_staging_counts_towards_total_quota_without_reading_its_bytes() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let raw = store.staging_dir(
        "quota-session",
        "quota-submission",
        "quota-reservation",
    );
    fs::create_dir_all(&raw).unwrap();
    let payload = raw.join("payload");
    create_sparse_file(&payload, super::MAX_STORE_BYTES);
    assert_eq!(store.usage().unwrap().staging_bytes, super::MAX_STORE_BYTES);
    fs::write(
        scratch.0.join("workspace/outputs/report.csv"),
        b"a,b\n1,2\n",
    )
    .unwrap();
    let error = store
        .prepare(request(&scratch.0, "outputs/report.csv", "quota-overflow"))
        .unwrap_err();
    assert!(error.to_string().contains("storage limit"));
    let usage = store.usage().unwrap();
    assert_eq!(usage.staging_bytes, super::MAX_STORE_BYTES);
    assert_eq!(usage.stored_bytes, 0);
    assert_eq!(usage.stored_items, 1);
    assert_eq!(fs::metadata(payload).unwrap().len(), super::MAX_STORE_BYTES);
    assert_eq!(store.scan_locked().unwrap().manifests.as_slice(), []);
}

#[test]
fn missing_empty_directory_and_mismatched_pdf_sources_never_prepare_a_delivery()
{
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::File::create(scratch.0.join("workspace/outputs/empty.csv")).unwrap();
    fs::create_dir(scratch.0.join("workspace/outputs/folder")).unwrap();
    fs::write(
        scratch.0.join("workspace/outputs/pretend.pdf"),
        b"This is not PDF data.",
    )
    .unwrap();
    for (relative, key) in [
        ("outputs/missing.csv", "missing"),
        ("outputs/empty.csv", "empty"),
        ("outputs/folder", "directory"),
        ("outputs/pretend.pdf", "header"),
    ] {
        assert!(
            store.prepare(request(&scratch.0, relative, key)).is_err(),
            "invalid source {key} was accepted"
        );
    }
    assert_eq!(store.list().unwrap().as_slice(), []);
    assert_eq!(store.usage().unwrap().staging_bytes, 0);
    assert_eq!(store.usage().unwrap().stored_items, 0);
    assert_eq!(store.scan_locked().unwrap().manifests.as_slice(), []);
}

#[test]
fn prepared_artifact_is_hidden_until_delivery_commit() {
    let scratch = Scratch::new();
    fs::write(
        scratch.0.join("workspace/outputs/report.csv"),
        b"a,b\n1,2\n",
    )
    .unwrap();
    let store = store(&scratch.0);
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.csv", "report"))
        .unwrap();
    assert_eq!(store.list().unwrap().as_slice(), []);
    store.commit_delivery(&commit(&receipt)).unwrap();
    let rows = store.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].manifest.receipt.artifact_id, receipt.artifact_id);
    assert_eq!(
        store.read_payload(&receipt.artifact_id).unwrap(),
        b"a,b\n1,2\n"
    );
}

#[test]
fn same_delivery_key_reuses_identical_content_and_rejects_changes() {
    let scratch = Scratch::new();
    let path = scratch.0.join("workspace/outputs/report.md");
    fs::write(&path, b"# report\n").unwrap();
    let store = store(&scratch.0);
    let first = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    let retry = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    assert_eq!(first, retry);
    fs::write(path, b"# changed\n").unwrap();
    assert!(
        store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn imports_artifact_through_a_redirected_output_ancestor() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let report = b"# redirected report\n";
    fs::write(scratch.0.join("workspace/outputs/report.md"), report).unwrap();
    let redirected = scratch.0.join("redirected");
    std::os::unix::fs::symlink(scratch.0.join("workspace"), &redirected)
        .unwrap();

    let mut import = request(&scratch.0, "report.md", "report");
    import.source_root = redirected.join("outputs");
    let receipt = store.prepare(import).unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    assert_eq!(store.read_payload(&receipt.artifact_id).unwrap(), report);
}

#[test]
fn rejects_traversal_and_symlinked_sources() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    assert!(
        store
            .prepare(request(&scratch.0, "../outside", "bad"))
            .is_err()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let external = scratch.0.join("secret.md");
        fs::write(&external, b"secret").unwrap();
        symlink(&external, scratch.0.join("workspace/outputs/link.md"))
            .unwrap();
        assert!(
            store
                .prepare(request(&scratch.0, "outputs/link.md", "link"))
                .is_err()
        );
    }
}

#[test]
fn preferences_and_tombstones_survive_reopen_without_resurrection() {
    let scratch = Scratch::new();
    fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
        .unwrap();
    let store = store(&scratch.0);
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    store.set_favorite(&receipt.artifact_id, true).unwrap();
    store.trash(&receipt.artifact_id).unwrap();
    store.permanently_delete(&receipt.artifact_id).unwrap();
    drop(store);
    let reopened = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
    assert_eq!(reopened.list().unwrap().as_slice(), []);
    assert!(reopened.read_payload(&receipt.artifact_id).is_err());
}

#[test]
fn permanent_deletion_rejects_active_files_and_late_confirmations_after_restore()
 {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(
        scratch.0.join("workspace/outputs/report.md"),
        b"protected report",
    )
    .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    let original_manifest =
        fs::read(store.item_dir(&receipt.artifact_id).join("manifest.json"))
            .unwrap();
    assert!(store.permanently_delete(&receipt.artifact_id).is_err());
    assert_eq!(
        store.read_payload(&receipt.artifact_id).unwrap(),
        b"protected report"
    );
    assert_eq!(
        fs::read(store.item_dir(&receipt.artifact_id).join("manifest.json"))
            .unwrap(),
        original_manifest
    );
    store.trash(&receipt.artifact_id).unwrap();
    store.restore_from_trash(&receipt.artifact_id).unwrap();
    // A dialog opened while trashed may resolve after a separate restore.
    assert!(store.permanently_delete(&receipt.artifact_id).is_err());
    let restored = store.get(&receipt.artifact_id).unwrap().unwrap();
    assert_eq!(
        restored.manifest.publication_state,
        PublicationState::Committed
    );
    assert!(restored.preference.trashed_at_ms.is_none());
    assert_eq!(
        store.read_payload(&receipt.artifact_id).unwrap(),
        b"protected report"
    );
    assert_eq!(
        fs::read(store.item_dir(&receipt.artifact_id).join("manifest.json"))
            .unwrap(),
        original_manifest
    );
    store.trash(&receipt.artifact_id).unwrap();
    store.permanently_delete(&receipt.artifact_id).unwrap();
    let tombstone_path =
        store.item_dir(&receipt.artifact_id).join("manifest.json");
    let tombstone = fs::read(&tombstone_path).unwrap();
    assert_eq!(
        store
            .get(&receipt.artifact_id)
            .unwrap()
            .unwrap()
            .manifest
            .publication_state,
        PublicationState::Deleted
    );
    assert!(!store.payload_path(&receipt.artifact_id).exists());
    store.permanently_delete(&receipt.artifact_id).unwrap();
    assert_eq!(fs::read(tombstone_path).unwrap(), tombstone);
}

#[test]
fn permanent_deletion_requires_a_delivery_marker_and_never_promotes_prepared_files()
 {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(
        scratch.0.join("workspace/outputs/report.md"),
        b"prepared report",
    )
    .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    let staging = store.staging_dir(
        &receipt.session_id,
        &receipt.submission_id,
        &receipt.artifact_id,
    );
    fs::rename(&staging, store.item_dir(&receipt.artifact_id)).unwrap();
    fs::rename(
        store.item_dir(&receipt.artifact_id).join("prepared.json"),
        store.item_dir(&receipt.artifact_id).join("manifest.json"),
    )
    .unwrap();
    let path = store.item_dir(&receipt.artifact_id).join("manifest.json");
    let prepared_bytes = fs::read(&path).unwrap();
    assert!(store.permanently_delete(&receipt.artifact_id).is_err());
    assert_eq!(fs::read(&path).unwrap(), prepared_bytes);
    let mut manifest = super::read_manifest(&path).unwrap();
    manifest.publication_state = PublicationState::Committed;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let unmatched_bytes = fs::read(&path).unwrap();
    assert!(store.permanently_delete(&receipt.artifact_id).is_err());
    assert_eq!(fs::read(&path).unwrap(), unmatched_bytes);
    assert_eq!(
        fs::read(store.payload_path(&receipt.artifact_id)).unwrap(),
        b"prepared report"
    );
}

#[test]
fn orphaned_partial_snapshots_and_view_copies_are_charged() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::create_dir_all(store.root().join("staging/orphan/turn/file")).unwrap();
    fs::write(
        store.root().join("staging/orphan/turn/file/payload"),
        b"partial",
    )
    .unwrap();
    fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
        .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    let view = store.create_view_copy(&receipt.artifact_id).unwrap();
    let usage = store.usage().unwrap();
    assert_eq!(usage.staging_bytes, 7);
    assert_eq!(usage.stored_bytes, receipt.size_bytes);
    assert_eq!(usage.view_bytes, receipt.size_bytes);
    assert_eq!(usage.stored_items, 2);
    fs::write(&view.path, b"external edit").unwrap();
    assert_eq!(
        store.read_payload(&receipt.artifact_id).unwrap(),
        b"# report\n"
    );
    store.remove_view_copy(&view.request_id).unwrap();
    assert_eq!(store.usage().unwrap().view_bytes, 0);
}

#[test]
fn partial_group_commit_is_invisible_and_recovers_as_a_whole() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(scratch.0.join("workspace/outputs/one.md"), b"one").unwrap();
    fs::write(scratch.0.join("workspace/outputs/two.md"), b"two").unwrap();
    let one = store
        .prepare(request(&scratch.0, "outputs/one.md", "one"))
        .unwrap();
    let two = store
        .prepare(request(&scratch.0, "outputs/two.md", "two"))
        .unwrap();
    fs::rename(
        store.staging_dir(
            &one.session_id,
            &one.submission_id,
            &one.artifact_id,
        ),
        store.item_dir(&one.artifact_id),
    )
    .unwrap();
    assert_eq!(store.list().unwrap().as_slice(), []);
    let mut proof = commit(&one);
    proof.artifact_ids.push(two.artifact_id);
    store.commit_delivery(&proof).unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    proof.committed_at_ms += 1;
    proof.origin_title_snapshot = "later source title".into();
    store.commit_delivery(&proof).unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    let refreshed = store.get(&one.artifact_id).unwrap().unwrap();
    assert_eq!(
        refreshed.manifest.origin_title_snapshot,
        "later source title"
    );
    assert_eq!(
        refreshed.manifest.receipt.origin_title_snapshot,
        "later source title"
    );
}

#[test]
fn list_uses_metadata_while_read_verifies_the_digest() {
    let scratch = Scratch::new();
    let contents = b"# report
";
    fs::write(scratch.0.join("workspace/outputs/report.md"), contents).unwrap();
    let store = store(&scratch.0);
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    let record = store.get(&receipt.artifact_id).unwrap().unwrap();
    assert!(record.available);
    assert!(!record.corrupt);

    // Same-sized tampering is intentionally not detected during listing;
    // reading the payload still performs the full SHA-256 check.
    fs::write(
        store.payload_path(&receipt.artifact_id),
        vec![b'x'; contents.len()],
    )
    .unwrap();
    let listed = store.get(&receipt.artifact_id).unwrap().unwrap();
    assert!(listed.available);
    assert!(!listed.corrupt);
    assert!(store.read_payload(&receipt.artifact_id).is_err());

    // Metadata-level corruption remains distinguishable from a missing file.
    fs::write(store.payload_path(&receipt.artifact_id), b"tampered").unwrap();
    let corrupt = store.get(&receipt.artifact_id).unwrap().unwrap();
    assert!(!corrupt.available);
    assert!(corrupt.corrupt);

    fs::remove_file(store.payload_path(&receipt.artifact_id)).unwrap();
    let missing = store.get(&receipt.artifact_id).unwrap().unwrap();
    assert!(!missing.available);
    assert!(!missing.corrupt);
}

#[test]
fn unknown_schema_is_preserved_and_does_not_hide_healthy_records() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
        .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    let bad = store.root().join("items/future");
    fs::create_dir(&bad).unwrap();
    let mut future = store.get(&receipt.artifact_id).unwrap().unwrap().manifest;
    future.schema_version = 999;
    future.receipt.artifact_id = "future".into();
    let bytes = serde_json::to_vec(&future).unwrap();
    fs::write(bad.join("manifest.json"), &bytes).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_ne!(store.diagnostics().unwrap().as_slice(), NO_DIAGNOSTICS);
    assert!(store.permanently_delete("future").is_err());
    assert_eq!(fs::read(bad.join("manifest.json")).unwrap(), bytes);
}

#[test]
fn delete_pending_retries_on_open_and_tombstone_metadata_remains_queryable() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
        .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    store.trash(&receipt.artifact_id).unwrap();
    let mut manifest =
        store.get(&receipt.artifact_id).unwrap().unwrap().manifest;
    manifest.publication_state = PublicationState::DeletePending;
    fs::write(
        store.item_dir(&receipt.artifact_id).join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    drop(store);
    let reopened = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
    let deleted = reopened.get(&receipt.artifact_id).unwrap().unwrap();
    assert_eq!(
        deleted.manifest.publication_state,
        PublicationState::Deleted
    );
    assert!(!deleted.available);
    assert_eq!(reopened.usage().unwrap().stored_bytes, 0);
    reopened.commit_delivery(&commit(&receipt)).unwrap();
    assert_eq!(reopened.list().unwrap().as_slice(), []);
}

fn age_prepared(
    store: &ArtifactStore,
    receipt: &flowy_agent_core::ArtifactReceipt,
    age_ms: u64,
) -> PathBuf {
    let directory = store.staging_dir(
        &receipt.session_id,
        &receipt.submission_id,
        &receipt.artifact_id,
    );
    let path = directory.join("prepared.json");
    let mut manifest = super::read_manifest(&path).unwrap();
    manifest.receipt.created_at_ms = super::now_ms().saturating_sub(age_ms);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    directory
}

#[test]
fn cleanup_requires_readable_history_retains_seven_days_and_protects_successful_proofs()
 {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(
        scratch.0.join("workspace/outputs/report.md"),
        b"retained data",
    )
    .unwrap();
    let old = store
        .prepare(request(&scratch.0, "outputs/report.md", "old-failure"))
        .unwrap();
    let recent = store
        .prepare(request(&scratch.0, "outputs/report.md", "recent-failure"))
        .unwrap();
    let protected = store
        .prepare(request(
            &scratch.0,
            "outputs/report.md",
            "successful-history",
        ))
        .unwrap();
    let old_dir = age_prepared(&store, &old, PREPARED_RETENTION_MS + 60_000);
    let recent_dir = age_prepared(
        &store,
        &recent,
        PREPARED_RETENTION_MS - 24 * 60 * 60 * 1_000,
    );
    let protected_dir =
        age_prepared(&store, &protected, PREPARED_RETENTION_MS + 60_000);
    let raw =
        store.staging_dir("raw-session", "raw-submission", "unproved-copy");
    let raw_protected =
        store.staging_dir("raw-session", "raw-submission", "proved-copy");
    for directory in [&raw, &raw_protected] {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("payload"), b"incomplete copy").unwrap();
    }
    let mut proof = commit(&protected);
    proof.artifact_ids.push("proved-copy".into());

    // A corrupt/unreadable history cannot authorize even raw-copy cleanup.
    store
        .cleanup_uncommitted(std::slice::from_ref(&proof), false)
        .unwrap();
    assert!(old_dir.exists());
    assert!(raw.exists());
    store
        .cleanup_uncommitted(std::slice::from_ref(&proof), true)
        .unwrap();
    assert!(!old_dir.exists());
    assert!(!raw.exists());
    assert!(recent_dir.join("payload").is_file());
    assert!(protected_dir.join("payload").is_file());
    assert!(raw_protected.join("payload").is_file());
}

#[test]
fn cleanup_never_ages_out_committed_payloads() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(
        scratch.0.join("workspace/outputs/report.md"),
        b"committed data",
    )
    .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    age_prepared(&store, &receipt, PREPARED_RETENTION_MS + 60_000);
    store.commit_delivery(&commit(&receipt)).unwrap();
    // Independent markers protect files even after their session is gone.
    store.cleanup_uncommitted(&[], true).unwrap();
    assert_eq!(
        store.read_payload(&receipt.artifact_id).unwrap(),
        b"committed data"
    );
}

#[test]
fn deleting_the_current_source_version_does_not_resurrect_an_old_one() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let source = scratch.0.join("workspace/outputs/report.md");
    fs::write(&source, b"old").unwrap();
    let first = store
        .prepare(request(&scratch.0, "outputs/report.md", "first"))
        .unwrap();
    store.commit_delivery(&commit(&first)).unwrap();
    fs::write(&source, b"new").unwrap();
    let mut next = request(&scratch.0, "outputs/report.md", "second");
    next.submission_id = "submission-2".into();
    let second = store.prepare(next).unwrap();
    store.commit_delivery(&commit(&second)).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);

    store.trash(&second.artifact_id).unwrap();
    store.permanently_delete(&second.artifact_id).unwrap();
    assert_eq!(store.list().unwrap().as_slice(), []);
    assert_eq!(store.read_payload(&first.artifact_id).unwrap(), b"old");
}

#[test]
fn corrupt_marker_isolates_its_group_but_blocks_cleanup_and_new_metadata() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(scratch.0.join("workspace/outputs/report.md"), b"real data")
        .unwrap();
    let bad = store
        .prepare(request(&scratch.0, "outputs/report.md", "first"))
        .unwrap();
    store.commit_delivery(&commit(&bad)).unwrap();
    let mut second_request = request(&scratch.0, "outputs/report.md", "second");
    second_request.submission_id = "submission-2".into();
    // Deliberately use a distinct source identity so this fixture still
    // exercises one healthy artifact alongside a corrupt marker.
    second_request.source_relative_path = "outputs/healthy.md".into();
    let healthy = store.prepare(second_request).unwrap();
    store.commit_delivery(&commit(&healthy)).unwrap();
    let raw = store.staging_dir("raw-session", "raw-submission", "raw-copy");
    fs::create_dir_all(&raw).unwrap();
    fs::write(raw.join("payload"), b"partial").unwrap();
    let marker_path = store.delivery_path(&bad.session_id, &bad.submission_id);
    fs::write(&marker_path, b"{broken marker").unwrap();
    let records = store.list().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.receipt.artifact_id, healthy.artifact_id);
    assert_eq!(
        store.read_payload(&healthy.artifact_id).unwrap(),
        b"real data"
    );
    assert!(store.read_payload(&bad.artifact_id).is_err());
    assert_ne!(store.diagnostics().unwrap().as_slice(), NO_DIAGNOSTICS);
    assert!(store.cleanup_uncommitted(&[], true).is_err());
    assert!(raw.join("payload").is_file());
    assert!(store.set_favorite(&healthy.artifact_id, true).is_err());
    assert_eq!(fs::read(marker_path).unwrap(), b"{broken marker");
}

#[test]
fn future_manifest_preflight_prevents_partial_cleanup() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    fs::write(
        scratch.0.join("workspace/outputs/report.md"),
        b"prepared data",
    )
    .unwrap();
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.md", "report"))
        .unwrap();
    let directory =
        age_prepared(&store, &receipt, PREPARED_RETENTION_MS + 60_000);
    let path = directory.join("prepared.json");
    let mut manifest = super::read_manifest(&path).unwrap();
    manifest.schema_version = 999;
    let bytes = serde_json::to_vec(&manifest).unwrap();
    fs::write(&path, &bytes).unwrap();
    let raw = store.staging_dir("raw-session", "raw-submission", "raw-copy");
    fs::create_dir_all(&raw).unwrap();
    fs::write(raw.join("payload"), b"partial").unwrap();
    assert!(store.cleanup_uncommitted(&[], true).is_err());
    assert!(raw.join("payload").is_file());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn atomic_metadata_replacement_updates_an_existing_destination() {
    let scratch = Scratch::new();
    let path = scratch.0.join("preferences.json");
    fs::write(&path, b"old bytes").unwrap();
    super::atomic_write(&path, b"new bytes").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"new bytes");
    assert!(fs::read_dir(&scratch.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".artifact-")
    }));
}

#[cfg(windows)]
#[test]
fn occupied_metadata_replacement_keeps_original_bytes_and_no_backup_gap() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new();
    let path = scratch.0.join("preferences.json");
    fs::write(&path, b"old bytes").unwrap();
    let occupied = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(super::atomic_write(&path, b"new bytes").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"old bytes");
    assert!(fs::read_dir(&scratch.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".artifact-")
    }));
    drop(occupied);
    super::atomic_write(&path, b"new bytes").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"new bytes");
}

#[test]
fn session_deletion_hides_then_removes_all_managed_records() {
    let scratch = Scratch::new();
    fs::write(
        scratch.0.join("workspace/outputs/report.csv"),
        b"value\n1\n",
    )
    .unwrap();
    let store = store(&scratch.0);
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.csv", "report"))
        .unwrap();
    let delivery = commit(&receipt);
    store.commit_delivery(&delivery).unwrap();
    store.set_favorite(&receipt.artifact_id, true).unwrap();
    let staging = store.staging_dir(
        &delivery.session_id,
        &delivery.submission_id,
        "orphan",
    );
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("payload"), b"partial").unwrap();
    fs::write(
        store.source_deletion_path(&delivery.session_id),
        br#"{"schema_version":1,"session_id":"session-1","deleted_at_ms":1}"#,
    )
    .unwrap();

    store
        .prepare_session_deletion(
            &delivery.session_id,
            &delivery.artifact_ids,
            std::slice::from_ref(&delivery.submission_id),
        )
        .unwrap();
    assert_eq!(store.list().unwrap().as_slice(), []);
    assert!(store.get(&receipt.artifact_id).unwrap().is_none());

    store.cancel_session_deletion(&delivery.session_id).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);

    store
        .prepare_session_deletion(
            &delivery.session_id,
            &delivery.artifact_ids,
            std::slice::from_ref(&delivery.submission_id),
        )
        .unwrap();
    store.commit_session_deletion(&delivery.session_id).unwrap();
    store
        .cleanup_session_deletion(&delivery.session_id)
        .unwrap();
    store
        .complete_session_deletion(&delivery.session_id)
        .unwrap();

    assert!(!store.item_dir(&receipt.artifact_id).exists());
    assert!(
        !store
            .delivery_path(&delivery.session_id, &delivery.submission_id)
            .exists()
    );
    assert!(!staging.exists());
    assert!(!store.source_deletion_path(&delivery.session_id).exists());
    assert!(!store.session_deletion_path(&delivery.session_id).exists());
    assert_eq!(store.list().unwrap().as_slice(), []);
    assert!(store.preferences_locked().unwrap().is_empty());
}

#[test]
fn session_deletion_is_idempotent_and_survives_partial_cleanup() {
    let scratch = Scratch::new();
    fs::write(
        scratch.0.join("workspace/outputs/report.csv"),
        b"value\n1\n",
    )
    .unwrap();
    let store = store(&scratch.0);
    let receipt = store
        .prepare(request(&scratch.0, "outputs/report.csv", "report"))
        .unwrap();
    let delivery = commit(&receipt);
    store.commit_delivery(&delivery).unwrap();
    store
        .prepare_session_deletion(
            &delivery.session_id,
            &delivery.artifact_ids,
            std::slice::from_ref(&delivery.submission_id),
        )
        .unwrap();
    store.commit_session_deletion(&delivery.session_id).unwrap();

    // Simulate a crash after part of the cleanup became durable.
    fs::remove_file(store.payload_path(&receipt.artifact_id)).unwrap();
    assert_eq!(store.session_deletions().unwrap().len(), 1);
    assert_eq!(store.list().unwrap().as_slice(), []);
    store
        .cleanup_session_deletion(&delivery.session_id)
        .unwrap();
    assert_eq!(store.session_deletions().unwrap().len(), 1);
    store
        .complete_session_deletion(&delivery.session_id)
        .unwrap();
    assert!(!store.item_dir(&receipt.artifact_id).exists());
    assert!(store.session_deletions().unwrap().is_empty());
    assert!(
        store
            .cleanup_session_deletion(&delivery.session_id)
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn occupied_uncommitted_cleanup_keeps_actual_quota_until_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let raw = store.staging_dir("raw-session", "raw-submission", "raw-copy");
    fs::create_dir_all(&raw).unwrap();
    let payload = raw.join("payload");
    fs::write(&payload, b"partial").unwrap();
    let occupied = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&payload)
        .unwrap();
    assert!(store.cleanup_uncommitted(&[], true).is_err());
    assert_eq!(store.usage().unwrap().staging_bytes, 7);
    assert_ne!(store.diagnostics().unwrap().as_slice(), NO_DIAGNOSTICS);
    assert_eq!(fs::read(&payload).unwrap(), b"partial");
    drop(occupied);
    store.cleanup_uncommitted(&[], true).unwrap();
    assert_eq!(store.usage().unwrap().staging_bytes, 0);
}

fn prepare_and_commit_markdown(
    scratch: &Path,
    store: &ArtifactStore,
    name: &str,
    body: &[u8],
) -> flowy_agent_core::ArtifactReceipt {
    let rel = format!("outputs/{name}");
    fs::write(scratch.join("workspace").join(&rel), body).unwrap();
    let receipt = store.prepare(request(scratch, &rel, name)).unwrap();
    store.commit_delivery(&commit(&receipt)).unwrap();
    receipt
}

fn read_view_meta(store: &ArtifactStore, request_id: &str) -> ViewCopyMeta {
    let bytes = fs::read(store.view_meta_path(request_id)).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn view_copy_gc_retains_newest_five_and_evicts_oldest_two() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "report.md",
        b"# report\n",
    );

    let mut copies = Vec::new();
    for _ in 0..7 {
        copies.push(store.create_view_copy(&receipt.artifact_id).unwrap());
    }

    for evicted in &copies[..2] {
        assert!(
            !store
                .root()
                .join("views")
                .join(&evicted.request_id)
                .exists()
        );
        assert!(!store.view_meta_path(&evicted.request_id).exists());
    }
    for retained in &copies[2..] {
        assert!(retained.path.is_file());
        assert!(store.view_meta_path(&retained.request_id).is_file());
    }

    let pending = store.pending_views().unwrap();
    assert_eq!(pending.len(), MAX_RETAINED_VIEW_COPIES);
    let pending_ids = pending
        .iter()
        .map(|copy| copy.request_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let expected_ids = copies[2..]
        .iter()
        .map(|copy| copy.request_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(pending_ids, expected_ids);
    assert!(
        pending
            .iter()
            .all(|copy| copy.path.starts_with(store.root().join("views")))
    );

    // Manually seed 2 older valid managed copies directly on disk and
    // verify deterministic GC evicts exactly 2 and keeps the newest 5.
    let oldest_retained_ts =
        read_view_meta(&store, &copies[2].request_id).created_at_ms;
    for (offset, req_id) in [(10_u64, "manual-old-1"), (5_u64, "manual-old-2")]
    {
        let dir = store.root().join("views").join(req_id);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("artifact.md");
        fs::write(&file_path, b"# report\n").unwrap();
        let meta = ViewCopyMeta {
            request_id: req_id.to_owned(),
            artifact_id: receipt.artifact_id.clone(),
            file_name: "artifact.md".to_owned(),
            sha256: super::sha256_hex(b"# report\n"),
            created_at_ms: oldest_retained_ts.saturating_sub(offset),
        };
        fs::write(
            store.view_meta_path(req_id),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        store
            .cleanup_stale_view_copies_at(oldest_retained_ts + 1)
            .unwrap(),
        2
    );
    assert!(!store.root().join("views/manual-old-1").exists());
    assert!(!store.root().join("views/manual-old-2").exists());
    assert!(!store.view_meta_path("manual-old-1").exists());
    assert!(!store.view_meta_path("manual-old-2").exists());
    assert_eq!(
        store.pending_views().unwrap().len(),
        MAX_RETAINED_VIEW_COPIES
    );
}

#[test]
fn view_copy_gc_evicts_copies_at_or_beyond_three_days_under_cap() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "summary.md",
        b"summary body",
    );

    let first = store.create_view_copy(&receipt.artifact_id).unwrap();
    let second = store.create_view_copy(&receipt.artifact_id).unwrap();
    let first_ts = read_view_meta(&store, &first.request_id).created_at_ms;
    let second_ts = read_view_meta(&store, &second.request_id).created_at_ms;

    assert_eq!(
        store
            .cleanup_stale_view_copies_at(first_ts + VIEW_COPY_RETENTION_MS - 1)
            .unwrap(),
        0
    );
    assert!(first.path.is_file());
    assert!(second.path.is_file());

    assert_eq!(
        store
            .cleanup_stale_view_copies_at(second_ts + VIEW_COPY_RETENTION_MS)
            .unwrap(),
        2
    );
    assert!(!store.root().join("views").join(&first.request_id).exists());
    assert!(!store.root().join("views").join(&second.request_id).exists());
    assert!(!store.view_meta_path(&first.request_id).exists());
    assert!(!store.view_meta_path(&second.request_id).exists());
    assert_eq!(store.pending_views().unwrap().as_slice(), []);
}

#[test]
fn view_copy_gc_preserves_tampered_copies_even_when_stale() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "notes.md",
        b"original bytes",
    );

    let edited = store.create_view_copy(&receipt.artifact_id).unwrap();
    fs::write(&edited.path, b"user modified bytes").unwrap();

    let mut others = Vec::new();
    for _ in 0..6 {
        others.push(store.create_view_copy(&receipt.artifact_id).unwrap());
    }

    // Oldest unmodified copy (others[0]) was evicted at rank 5, while the
    // older user-modified copy is preserved along with its sidecar.
    assert!(
        !store
            .root()
            .join("views")
            .join(&others[0].request_id)
            .exists()
    );
    assert!(!store.view_meta_path(&others[0].request_id).exists());
    assert_eq!(fs::read(&edited.path).unwrap(), b"user modified bytes");
    assert!(store.view_meta_path(&edited.request_id).is_file());

    // Even past 3 days, the modified copy is preserved while the 5 remaining
    // unmodified copies are evicted.
    let latest_ts = read_view_meta(&store, &others[5].request_id).created_at_ms;
    assert_eq!(
        store
            .cleanup_stale_view_copies_at(
                latest_ts + VIEW_COPY_RETENTION_MS + 1_000
            )
            .unwrap(),
        5
    );
    assert_eq!(fs::read(&edited.path).unwrap(), b"user modified bytes");
    assert!(store.view_meta_path(&edited.request_id).is_file());
    for copy in &others[1..] {
        assert!(!store.root().join("views").join(&copy.request_id).exists());
        assert!(!store.view_meta_path(&copy.request_id).exists());
    }
}

#[test]
fn view_copy_gc_preserves_legacy_and_corrupt_entries_and_runs_on_open() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "deck.md",
        b"deck bytes",
    );

    // Legacy view copy without sidecar.
    let legacy_dir = store.root().join("views/legacy-copy");
    fs::create_dir_all(&legacy_dir).unwrap();
    let legacy_file = legacy_dir.join("artifact.md");
    fs::write(&legacy_file, b"legacy view").unwrap();

    // Managed copy with an extra user-placed file in its directory.
    let with_extra = store.create_view_copy(&receipt.artifact_id).unwrap();
    let extra_file = with_extra.path.parent().unwrap().join("notes.txt");
    fs::write(&extra_file, b"extra user file").unwrap();
    let mut with_extra_meta = read_view_meta(&store, &with_extra.request_id);
    with_extra_meta.created_at_ms = super::now_ms()
        .saturating_sub(VIEW_COPY_RETENTION_MS.saturating_add(10_000));
    fs::write(
        store.view_meta_path(&with_extra.request_id),
        serde_json::to_vec(&with_extra_meta).unwrap(),
    )
    .unwrap();

    // Corrupt sidecar and mismatched-ID sidecar.
    let corrupt_dir = store.root().join("views/corrupt-copy");
    fs::create_dir_all(&corrupt_dir).unwrap();
    fs::write(corrupt_dir.join("artifact.md"), b"deck bytes").unwrap();
    fs::write(store.view_meta_path("corrupt-copy"), b"{broken json").unwrap();

    let mismatch_dir = store.root().join("views/mismatch-copy");
    fs::create_dir_all(&mismatch_dir).unwrap();
    fs::write(mismatch_dir.join("artifact.md"), b"deck bytes").unwrap();
    fs::write(
        store.view_meta_path("mismatch-copy"),
        serde_json::to_vec(&ViewCopyMeta {
            request_id: "other-id".to_owned(),
            artifact_id: receipt.artifact_id.clone(),
            file_name: "artifact.md".to_owned(),
            sha256: super::sha256_hex(b"deck bytes"),
            created_at_ms: 1,
        })
        .unwrap(),
    )
    .unwrap();

    // Orphaned sidecar without a directory under views/.
    fs::write(
        store.view_meta_path("orphan-copy"),
        serde_json::to_vec(&ViewCopyMeta {
            request_id: "orphan-copy".to_owned(),
            artifact_id: receipt.artifact_id.clone(),
            file_name: "artifact.md".to_owned(),
            sha256: super::sha256_hex(b"deck bytes"),
            created_at_ms: 1,
        })
        .unwrap(),
    )
    .unwrap();

    // Expired unmodified managed copy that should be evicted on open().
    let expired = store.create_view_copy(&receipt.artifact_id).unwrap();
    let mut expired_meta = read_view_meta(&store, &expired.request_id);
    expired_meta.created_at_ms = super::now_ms()
        .saturating_sub(VIEW_COPY_RETENTION_MS.saturating_add(10_000));
    fs::write(
        store.view_meta_path(&expired.request_id),
        serde_json::to_vec(&expired_meta).unwrap(),
    )
    .unwrap();

    drop(store);
    let reopened = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();

    assert!(
        !reopened
            .root()
            .join("views")
            .join(&expired.request_id)
            .exists()
    );
    assert!(!reopened.view_meta_path(&expired.request_id).exists());
    assert!(!reopened.view_meta_path("orphan-copy").exists());

    assert_eq!(fs::read(&legacy_file).unwrap(), b"legacy view");
    assert!(with_extra.path.is_file());
    assert_eq!(fs::read(&extra_file).unwrap(), b"extra user file");
    assert!(corrupt_dir.join("artifact.md").is_file());
    assert!(mismatch_dir.join("artifact.md").is_file());
}

#[test]
fn remove_view_copy_removes_dir_and_sidecar_and_pending_views_ignores_meta() {
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "brief.md",
        b"brief content",
    );

    let view = store.create_view_copy(&receipt.artifact_id).unwrap();
    assert!(view.path.is_file());
    assert!(store.view_meta_path(&view.request_id).is_file());

    let pending = store.pending_views().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, view.request_id);
    assert_eq!(pending[0].path, view.path);

    store.remove_view_copy(&view.request_id).unwrap();
    assert!(!store.root().join("views").join(&view.request_id).exists());
    assert!(!store.view_meta_path(&view.request_id).exists());
    assert_eq!(store.pending_views().unwrap().as_slice(), []);

    // Idempotent second removal.
    store.remove_view_copy(&view.request_id).unwrap();
}

#[cfg(windows)]
#[test]
fn occupied_stale_view_copy_is_skipped_without_blocking_other_evictions() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new();
    let store = store(&scratch.0);
    let receipt = prepare_and_commit_markdown(
        &scratch.0,
        &store,
        "locked.md",
        b"locked bytes",
    );

    let locked = store.create_view_copy(&receipt.artifact_id).unwrap();
    let unlocked = store.create_view_copy(&receipt.artifact_id).unwrap();
    let latest_ts = read_view_meta(&store, &unlocked.request_id).created_at_ms;

    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&locked.path)
        .unwrap();
    assert_eq!(
        store
            .cleanup_stale_view_copies_at(
                latest_ts + VIEW_COPY_RETENTION_MS + 1_000
            )
            .unwrap(),
        1
    );
    assert!(locked.path.is_file());
    assert!(store.view_meta_path(&locked.request_id).is_file());
    assert!(
        !store
            .root()
            .join("views")
            .join(&unlocked.request_id)
            .exists()
    );
    assert!(!store.view_meta_path(&unlocked.request_id).is_file());

    drop(handle);
    assert_eq!(
        store
            .cleanup_stale_view_copies_at(
                latest_ts + VIEW_COPY_RETENTION_MS + 1_000
            )
            .unwrap(),
        1
    );
    assert!(!store.root().join("views").join(&locked.request_id).exists());
    assert!(!store.view_meta_path(&locked.request_id).is_file());
}
