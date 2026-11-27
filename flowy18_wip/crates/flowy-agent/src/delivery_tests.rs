use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("flowy-delivery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn artifact(root: &Path, body: &[u8]) -> PathBuf {
    let path = root.join("deck.pptx");
    std::fs::write(&path, body).unwrap();
    path
}

fn counting_sender() -> (Arc<AtomicUsize>, impl Fn(&[u8]) -> io::Result<()>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let send = move |_bytes: &[u8]| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    (calls, send)
}

fn deliver_outcome<F>(
    store: &DeliveryStore,
    path: &Path,
    allow_override: bool,
    now_ms: u64,
    send: F,
) -> DeliveryOutcome
where
    F: FnOnce(&[u8]) -> io::Result<()>,
{
    store
        .deliver_with(path, allow_override, now_ms, send)
        .unwrap()
        .outcome
}

#[test]
fn a_missing_report_blocks_without_calling_the_sender() {
    let root = scratch("missing");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK deck");
    let (calls, send) = counting_sender();
    let outcome = deliver_outcome(&store, &path, false, 1_000, send);
    assert!(!outcome.delivered);
    assert_eq!(outcome.blocked, Some(BlockReason::MissingReport));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_report_blocks_and_keeps_the_issues() {
    let root = scratch("failed");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK deck");
    let issues = vec![Issue {
        code: "VALUE_MISMATCH".to_owned(),
        field: "S0-title".to_owned(),
    }];
    store.record_report(&path, issues, 1_000).unwrap();
    let (calls, send) = counting_sender();
    let outcome = deliver_outcome(&store, &path, false, 2_000, send);
    assert_eq!(outcome.blocked, Some(BlockReason::VerificationFailed));
    assert!(!outcome.delivered);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.latest_report(&path).unwrap().unwrap().issues.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_changed_artifact_no_longer_matches_its_report() {
    let root = scratch("changed");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK original");
    store.record_report(&path, Vec::new(), 1_000).unwrap();
    std::fs::write(&path, b"PK tampered").unwrap();
    let (calls, send) = counting_sender();
    let outcome = deliver_outcome(&store, &path, false, 2_000, send);
    assert_eq!(outcome.blocked, Some(BlockReason::HashMismatch));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_verified_artifact_is_delivered_with_the_reported_bytes() {
    let root = scratch("pass");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK verified");
    store.record_report(&path, Vec::new(), 1_000).unwrap();
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = sent.clone();
    let outcome = deliver_outcome(&store, &path, false, 2_000, move |bytes| {
        sink.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    });
    assert!(outcome.delivered);
    assert!(outcome.blocked.is_none());
    assert_eq!(sent.lock().unwrap().as_slice(), b"PK verified");
    assert!(outcome.report_id.is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn override_is_explicit_and_leaves_both_events() {
    let root = scratch("override");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK deck");
    let (calls, send) = counting_sender();
    let outcome = deliver_outcome(&store, &path, true, 1_000, send);
    assert!(outcome.delivered);
    assert!(outcome.override_used);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let expected_hash = hash_bytes(b"PK deck");
    assert!(
        store
            .overridden_artifact_hashes()
            .unwrap()
            .contains(&expected_hash)
    );
    assert!(store.artifact_was_overridden(&expected_hash).unwrap());
    let events = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events.contains("deliver_blocked"));
    assert!(events.contains("deliver_override"));
    assert!(events.contains("\"deliver\""));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_blocked_delivery_writes_no_deliver_event() {
    let root = scratch("audit");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK deck");
    let (calls, send) = counting_sender();
    let _ = deliver_outcome(&store, &path, false, 1_000, send);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let events = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events.contains("deliver_blocked"));
    assert!(!events.contains("\"deliver\""));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn normalized_paths_match_the_same_verification_report() {
    let root = scratch("norm");
    let store = DeliveryStore::new(root.join("delivery"));
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("deck.pptx");
    std::fs::write(&file, b"PK normalized").unwrap();
    let slash_form =
        PathBuf::from(file.display().to_string().replace('\\', "/"));
    store.record_report(&slash_form, Vec::new(), 1_000).unwrap();
    let dot_form = sub.join(".").join("deck.pptx");
    assert_eq!(store.check(&dot_form).unwrap(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exempt_extensions_pass_with_explicit_audit_while_unknown_extensions_block() {
    let root = scratch("exempt");
    let store = DeliveryStore::new(root.join("delivery"));
    let csv = root.join("sales.csv");
    std::fs::write(&csv, b"a,b\n1,2\n").unwrap();
    let (csv_calls, csv_send) = counting_sender();
    let csv_outcome = deliver_outcome(&store, &csv, false, 1_000, csv_send);
    assert!(csv_outcome.delivered);
    assert_eq!(csv_outcome.exemption_code, Some(EXEMPT_NO_VERIFIER_V0));
    assert_eq!(csv_calls.load(Ordering::SeqCst), 1);

    let unknown = root.join("bundle.bin");
    std::fs::write(&unknown, b"\x00\x01\x02").unwrap();
    let (bin_calls, bin_send) = counting_sender();
    let bin_outcome = deliver_outcome(&store, &unknown, false, 2_000, bin_send);
    assert!(!bin_outcome.delivered);
    assert_eq!(bin_outcome.blocked, Some(BlockReason::MissingReport));
    assert_eq!(bin_calls.load(Ordering::SeqCst), 0);

    let events = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events.contains("deliver_exempt"));
    assert!(events.contains(EXEMPT_NO_VERIFIER_V0));
    assert!(events.contains("deliver_blocked"));
    assert_eq!(store.in_doubt_deliveries().unwrap().as_slice(), []);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_sender_failure_leaves_deliver_intent_in_doubt_without_deliver() {
    let root = scratch("in-doubt");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK verified");
    store.record_report(&path, Vec::new(), 1_000).unwrap();
    let result = store.deliver_with(&path, false, 2_000, |_bytes| {
        Err::<(), _>(io::Error::other("simulated crash mid-send"))
    });
    assert!(result.is_err());
    let events = store.read_events().unwrap();
    let intent_id = events
        .iter()
        .find_map(|event| {
            if let AuditEvent::DeliverIntent { delivery_id, .. } = event {
                Some(delivery_id.clone())
            } else {
                None
            }
        })
        .expect("deliver_intent must be persisted before send");
    let unresolved = store.in_doubt_deliveries().unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].delivery_id, intent_id);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn report_override_blocks_missing_stale_and_tampered_artifacts() {
    let root = scratch("override-bound");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK original");
    let (calls, send) = counting_sender();

    // Missing report must be hard-blocked even on override.
    let missing = store
        .deliver_override_for_report(&path, "report-none", 1_000, send)
        .unwrap();
    assert!(!missing.outcome.delivered);
    assert_eq!(missing.outcome.blocked, Some(BlockReason::MissingReport));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Record a failed report for the original bytes.
    let failed = store
        .record_report_bytes(
            &path,
            b"PK original",
            vec![Issue {
                code: "STRUCTURE".to_owned(),
                field: "brief_min_slides".to_owned(),
            }],
            2_000,
        )
        .unwrap();

    // Stale report_id must be rejected.
    let (stale_calls, stale_send) = counting_sender();
    let stale = store
        .deliver_override_for_report(&path, "report-stale", 2_500, stale_send)
        .unwrap();
    assert!(!stale.outcome.delivered);
    assert_eq!(stale.outcome.blocked, Some(BlockReason::HashMismatch));
    assert_eq!(stale_calls.load(Ordering::SeqCst), 0);

    // Tampering with file bytes after the failed card opened must be rejected.
    std::fs::write(&path, b"PK swapped-bytes").unwrap();
    let (swap_calls, swap_send) = counting_sender();
    let swapped = store
        .deliver_override_for_report(&path, &failed.id, 3_000, swap_send)
        .unwrap();
    assert!(!swapped.outcome.delivered);
    assert_eq!(swapped.outcome.blocked, Some(BlockReason::HashMismatch));
    assert_eq!(swap_calls.load(Ordering::SeqCst), 0);

    // Restoring the exact bytes bound to `failed.id` allows explicit override.
    std::fs::write(&path, b"PK original").unwrap();
    let (ok_calls, ok_send) = counting_sender();
    let allowed = store
        .deliver_override_for_report(&path, &failed.id, 4_000, ok_send)
        .unwrap();
    assert!(allowed.outcome.delivered);
    assert!(allowed.outcome.override_used);
    assert_eq!(ok_calls.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stage_failure_aborts_intent_and_override_is_scoped_by_report() {
    let root = scratch("stage-abort-and-scope");
    let store = DeliveryStore::new(root.join("delivery"));
    let path_a = root.join("sess-a").join("deck.pptx");
    let path_b = root.join("sess-b").join("deck.pptx");
    std::fs::create_dir_all(path_a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(path_b.parent().unwrap()).unwrap();
    std::fs::write(&path_a, b"PK identical-bytes").unwrap();
    std::fs::write(&path_b, b"PK identical-bytes").unwrap();

    let rep_a = store
        .record_report(
            &path_a,
            vec![Issue {
                code: "STRUCTURE".into(),
                field: "slides".into(),
            }],
            1_000,
        )
        .unwrap();
    let rep_b = store
        .record_report(
            &path_b,
            vec![Issue {
                code: "STRUCTURE".into(),
                field: "slides".into(),
            }],
            1_100,
        )
        .unwrap();

    // 1) Local `stage_override_for_report` failure must abort the intent
    //    so no ghost in-doubt delivery remains.
    let failed =
        store.stage_override_for_report(&path_a, &rep_a.id, 1_200, |_| {
            Err::<(), _>(io::Error::other("local prepare failed"))
        });
    assert!(failed.is_err());
    assert!(
        store.in_doubt_deliveries().unwrap().is_empty(),
        "failed local staging must close DeliverIntent with DeliverAborted"
    );

    // 2) Overriding session A's report must NOT mark session B's report as
    //    overridden even when both files have identical SHA-256 bytes.
    let (_, send) = counting_sender();
    let delivered = store
        .deliver_override_for_report(&path_a, &rep_a.id, 1_300, send)
        .unwrap();
    assert!(delivered.outcome.delivered);
    assert!(
        store
            .report_was_overridden(
                &rep_a.id,
                &path_a.display().to_string(),
                &rep_a.artifact_hash,
            )
            .unwrap()
    );
    assert!(
        !store
            .report_was_overridden(
                &rep_b.id,
                &path_b.display().to_string(),
                &rep_b.artifact_hash,
            )
            .unwrap(),
        "session A override must not poison session B's failed card with same sha256"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stage_with_writes_intent_only_until_confirmed_idempotently() {
    let root = scratch("two-phase-stage");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK staged");
    store.record_report(&path, Vec::new(), 1_000).unwrap();
    let staged = store
        .stage_with(&path, false, 2_000, |bytes| Ok(hash_bytes(bytes)))
        .unwrap();
    assert!(!staged.outcome.delivered);
    assert!(staged.outcome.blocked.is_none());
    let hash = staged.value.expect("staged hash");
    let events_before = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events_before.contains("deliver_intent"));
    assert!(!events_before.contains("\"deliver\""));
    assert_eq!(store.in_doubt_deliveries().unwrap().len(), 1);

    assert_eq!(store.confirm_delivered_by_hash(&hash, 3_000).unwrap(), 1);
    assert_eq!(store.in_doubt_deliveries().unwrap().as_slice(), []);
    assert_eq!(store.confirm_delivered_by_hash(&hash, 4_000).unwrap(), 0);
    let events_after = std::fs::read_to_string(store.events_path()).unwrap();
    assert_eq!(events_after.matches("\"deliver\"").count(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn empty_report_id_is_hard_blocked_on_override() {
    let root = scratch("override-empty-id");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK empty-id");
    store
        .record_report_bytes(
            &path,
            b"PK empty-id",
            vec![Issue {
                code: "STRUCTURE".to_owned(),
                field: "slides".to_owned(),
            }],
            1_000,
        )
        .unwrap();
    let (calls, send) = counting_sender();
    let rejected = store
        .stage_override_for_report(&path, "   ", 2_000, send)
        .unwrap();
    assert!(!rejected.outcome.delivered);
    assert_eq!(rejected.outcome.blocked, Some(BlockReason::HashMismatch));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn interior_corruption_in_reports_jsonl_fails_closed() {
    let root = scratch("corrupt-reports");
    let store = DeliveryStore::new(root.join("delivery"));
    let deck = artifact(&root, b"PK deck");
    let other = root.join("other.pptx");
    std::fs::write(&other, b"PK other").unwrap();

    // Passing report for `deck`, then a corrupted interior line (simulating
    // a damaged later failing report), then another valid report for `other`.
    store.record_report(&deck, Vec::new(), 1_000).unwrap();
    let mut handle = std::fs::OpenOptions::new()
        .append(true)
        .open(store.reports_path())
        .unwrap();
    writeln!(handle, r#"{{"id":"corrupt","ok":false"#).unwrap();
    drop(handle);
    store.record_report(&other, Vec::new(), 2_000).unwrap();

    assert_eq!(store.latest_report(&deck).unwrap(), None);
    assert_eq!(
        store.check(&deck).unwrap(),
        Some(BlockReason::MissingReport)
    );
    // A valid report recorded AFTER the corruption still succeeds.
    assert!(store.latest_report(&other).unwrap().is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stale_contract_revision_report_is_blocked_at_delivery_gate() {
    let root = scratch("stale-contract-rev");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK rev1-verified");
    let report = store
        .record_report_bytes_with_revision(
            &path,
            b"PK rev1-verified",
            Vec::new(),
            Some(1),
            1_000,
        )
        .unwrap();
    assert_eq!(report.contract_revision, Some(1));
    assert!(report.ok);
    // A rev=1 ok=true report passes for rev=1 and legacy None checks, but
    // is hard-blocked once the contract advances to rev=2.
    assert_eq!(store.check_for_revision(&path, Some(1)).unwrap(), None);
    assert_eq!(store.check(&path).unwrap(), None);
    let blocked = store.check_for_revision(&path, Some(2)).unwrap();
    assert_eq!(blocked, Some(BlockReason::ContractRevisionMismatch));
    assert_eq!(
        blocked.unwrap().code(),
        "DELIVERY_CONTRACT_REVISION_MISMATCH"
    );
    let (calls, send) = counting_sender();
    let staged = store
        .stage_with_revision(&path, false, Some(2), 2_000, send)
        .unwrap();
    assert!(!staged.outcome.delivered);
    assert_eq!(
        staged.outcome.blocked,
        Some(BlockReason::ContractRevisionMismatch)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let events = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events.contains("DELIVERY_CONTRACT_REVISION_MISMATCH"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stage_override_for_stale_ok_report_logs_blocked_and_override_audit() {
    let root = scratch("override-stale-rev");
    let store = DeliveryStore::new(root.join("delivery"));
    let path = artifact(&root, b"PK rev1-ok-override");
    let report = store
        .record_report_bytes_with_revision(
            &path,
            b"PK rev1-ok-override",
            Vec::new(),
            Some(1),
            1_000,
        )
        .unwrap();
    assert!(report.ok);

    let (calls, send) = counting_sender();
    let staged = store
        .stage_override_for_report_with_revision(
            &path,
            &report.id,
            Some(2),
            2_000,
            send,
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(staged.outcome.override_used);
    assert_eq!(
        staged.outcome.blocked,
        Some(BlockReason::ContractRevisionMismatch)
    );
    let events = std::fs::read_to_string(store.events_path()).unwrap();
    assert!(events.contains("\"deliver_blocked\""));
    assert!(events.contains("\"deliver_override\""));
    assert!(events.contains("DELIVERY_CONTRACT_REVISION_MISMATCH"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reports_tail_cache_hits_tails_and_fails_closed_on_truncation_or_rewrite() {
    let root = scratch("reports-tail-cache");
    let store = DeliveryStore::new(root.join("delivery"));
    let pad = "a".repeat(256);
    let target = root.join("target.pptx");
    std::fs::write(&target, b"PK payload").unwrap();
    for i in 0..24_u64 {
        let other = root.join(format!("other-{i:02}.pptx"));
        std::fs::write(&other, b"PK other").unwrap();
        let path_for_rep = if i == 0 || i == 23 { &target } else { &other };
        let issues = if i == 0 {
            vec![Issue {
                code: "INIT".into(),
                field: pad.clone(),
            }]
        } else if i == 23 {
            Vec::new()
        } else {
            vec![Issue {
                code: "PAD".into(),
                field: pad.clone(),
            }]
        };
        store
            .record_report_bytes(path_for_rep, b"PK payload", issues, 1_000 + i)
            .unwrap();
    }
    let reports_path = store.reports_path();
    let original = std::fs::read(&reports_path).unwrap();
    assert!(original.len() > 8192);

    let _ = take_replay_stats();
    let first = store.latest_report(&target).unwrap().expect("report");
    assert!(first.ok);
    assert_eq!(take_replay_stats().full_replays, 1);

    // Cache hit on unchanged file
    let second = store.latest_report(&target).unwrap().expect("cached");
    assert_eq!(second, first);
    assert_eq!(
        take_replay_stats(),
        ReplayStats {
            cache_hits: 1,
            tail_parses: 0,
            full_replays: 0
        }
    );

    // Append a torn trailing fragment without `\n` -> tolerated via tail parse
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&reports_path)
            .unwrap();
        f.write_all(br#"{"id":"torn-"#).unwrap();
    }
    let with_torn = store.latest_report(&target).unwrap().expect("with torn");
    assert_eq!(with_torn, first);
    assert_eq!(take_replay_stats().tail_parses, 1);

    // Complete the torn fragment externally with a failing report for target
    let mut completed = original;
    let new_rep = Report {
        id: "rep-ext".into(),
        artifact_path: target.display().to_string(),
        artifact_hash: hash_bytes(b"PK payload"),
        ok: false,
        issues: vec![Issue {
            code: "EXT_FAIL".into(),
            field: "slide".into(),
        }],
        at_ms: 9_999,
        contract_revision: None,
        artifact_key: artifact_key(&target),
    };
    completed.extend_from_slice(&serde_json::to_vec(&new_rep).unwrap());
    completed.push(b'\n');
    std::fs::write(&reports_path, &completed).unwrap();

    let after_append = store.latest_report(&target).unwrap().expect("ext");
    assert!(!after_append.ok);
    assert_eq!(after_append.id, "rep-ext");
    assert_eq!(take_replay_stats().tail_parses, 1);

    // Same-length external rewrite in head bytes must fail closed -> full replay
    let mut head_mutated = completed.clone();
    let head_pos = head_mutated
        .windows(b"INIT".len())
        .position(|w| w == b"INIT")
        .expect("INIT in head");
    assert!(head_pos < 4096);
    head_mutated[head_pos..head_pos + 4].copy_from_slice(b"HEAD");
    assert_eq!(head_mutated.len(), completed.len());
    std::fs::write(&reports_path, &head_mutated).unwrap();
    let _ = store.latest_report(&target).unwrap();
    assert_eq!(take_replay_stats().full_replays, 1);

    // Same-length external rewrite in tail bytes must fail closed -> full replay
    let mut tail_mutated = head_mutated.clone();
    let tail_pos = tail_mutated
        .windows(b"EXT_FAIL".len())
        .position(|w| w == b"EXT_FAIL")
        .expect("EXT_FAIL in tail");
    assert!(tail_pos + 4096 >= tail_mutated.len());
    tail_mutated[tail_pos..tail_pos + 8].copy_from_slice(b"TAIL_ERR");
    assert_eq!(tail_mutated.len(), head_mutated.len());
    std::fs::write(&reports_path, &tail_mutated).unwrap();
    let after_tail = store.latest_report(&target).unwrap().expect("tail");
    assert_eq!(after_tail.issues[0].code, "TAIL_ERR");
    assert_eq!(take_replay_stats().full_replays, 1);

    // External truncation (`len < parsed_len`) must fail closed -> full replay
    let first_end =
        tail_mutated.iter().position(|&b| b == b'\n').expect("nl") + 1;
    std::fs::write(&reports_path, &tail_mutated[..first_end]).unwrap();
    let after_trunc = store.latest_report(&target).unwrap().expect("trunc");
    assert!(!after_trunc.ok);
    assert_eq!(after_trunc.issues[0].code, "HEAD");
    assert_eq!(take_replay_stats().full_replays, 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn events_tail_cache_hits_appends_and_invalidates_on_write_and_truncation() {
    let root = scratch("events-tail-cache");
    let store = DeliveryStore::new(root.join("delivery"));
    let pad = "e".repeat(256);
    for i in 0..24_u64 {
        DeliveryStore::append_json(
            &store.events_path(),
            &AuditEvent::DeliverIntent {
                delivery_id: format!("del-{i:02}-{pad}"),
                artifact_path: format!("deck-{i}.pptx"),
                artifact_hash: format!("hash-{i}"),
                at_ms: 1_000 + i,
            },
        )
        .unwrap();
    }
    let events_path = store.events_path();
    assert!(std::fs::metadata(&events_path).unwrap().len() > 8192);

    let _ = take_replay_stats();
    let first = store.read_events().unwrap();
    assert_eq!(first.len(), 24);
    assert_eq!(take_replay_stats().full_replays, 1);

    // Unchanged read hits cache directly.
    let cached = store.read_events().unwrap();
    assert_eq!(cached.len(), 24);
    assert_eq!(take_replay_stats().cache_hits, 1);

    // External append uses incremental tail parse.
    let extra = serde_json::to_vec(&AuditEvent::Deliver {
        delivery_id: format!("del-00-{pad}"),
        artifact_path: "deck-0.pptx".into(),
        artifact_hash: "hash-0".into(),
        at_ms: 2_000,
    })
    .unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&events_path)
            .unwrap();
        f.write_all(&extra).unwrap();
        f.write_all(b"\n").unwrap();
    }
    let after_ext = store.read_events().unwrap();
    assert_eq!(after_ext.len(), 25);
    assert_eq!(take_replay_stats().tail_parses, 1);

    // Write via DeliveryStore (`confirm_delivered`) invalidates cache so
    // stale state is never served.
    assert!(
        store
            .confirm_delivered(
                &format!("del-01-{pad}"),
                "deck-1.pptx",
                "hash-1",
                3_000,
            )
            .unwrap()
    );
    // `confirm_delivered` read via cache hit (len == parsed_len) before
    // appending and invalidating; next read must do full replay and see 26.
    let _ = take_replay_stats();
    let after_write = store.read_events().unwrap();
    assert_eq!(after_write.len(), 26);
    assert_eq!(take_replay_stats().full_replays, 1);

    // Truncate externally -> fail-closed full replay.
    let bytes = std::fs::read(&events_path).unwrap();
    let one_line = bytes.iter().position(|&b| b == b'\n').unwrap() + 1;
    std::fs::write(&events_path, &bytes[..one_line]).unwrap();
    let after_trunc = store.read_events().unwrap();
    assert_eq!(after_trunc.len(), 1);
    assert_eq!(take_replay_stats().full_replays, 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn delivery_tail_cache_stays_bounded_under_max_entries() {
    let mut roots = Vec::new();
    for i in 0..80_usize {
        let root = scratch(&format!("cache-bound-{i}"));
        let store = DeliveryStore::new(root.join("delivery"));
        let art = artifact(&root, b"PK bound");
        store.record_report(&art, Vec::new(), 1_000).unwrap();
        let _ = store.latest_report(&art).unwrap();
        roots.push(root);
    }
    let len = cache_registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len();
    assert!(
        len <= MAX_CACHE_ENTRIES,
        "cache size {len} exceeded bound {MAX_CACHE_ENTRIES}"
    );
    for root in roots {
        let _ = std::fs::remove_dir_all(&root);
    }
}
