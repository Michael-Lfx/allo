use super::*;

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "flowy-acceptance-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("temp root");
    root
}

#[test]
fn accept_survives_a_fresh_queue_handle() {
    let root = temp_root("durable");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue
        .accept(Some("home"), "second prompt", 1_000)
        .expect("accept");

    // A new handle sees the same receipt: this is the "restart and still
    // find it" contract, exercised without the UI.
    let reopened = AcceptanceQueue::new(&root);
    let pending = reopened.pending().expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, accepted.id);
    assert_eq!(pending[0].text, "second prompt");
    assert_eq!(pending[0].session_hint.as_deref(), Some("home"));
    assert!(pending[0].taken_at_ms.is_none());

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn taking_once_removes_it_from_pending_but_keeps_the_record() {
    let root = temp_root("take");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "only once", 1_000).expect("accept");

    let first = queue
        .take(&accepted.id, "attempt-1", 2_000, 30_000)
        .expect("take");
    assert!(first.is_some());
    let claimed = first.expect("claimed");
    assert_eq!(claimed.taken_at_ms, Some(2_000));
    assert_eq!(claimed.attempts, 1);
    assert!(claimed.available_at_ms.is_none());
    assert_eq!(
        claimed
            .lease
            .as_ref()
            .map(|lease| lease.attempt_id.as_str()),
        Some("attempt-1")
    );

    let second = queue
        .take(&accepted.id, "attempt-2", 3_000, 30_000)
        .expect("take twice");
    assert!(second.is_none(), "a receipt must not be run twice");

    assert_eq!(
        queue.pending_at(3_000).expect("pending").len(),
        0,
        "an unexpired lease keeps the receipt out of pending"
    );
    assert_eq!(queue.all().expect("all").len(), 1);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn empty_queue_reads_as_empty_not_missing() {
    let root = temp_root("empty");
    let queue = AcceptanceQueue::new(&root);
    assert_eq!(queue.pending().expect("pending"), Vec::new());
    assert!(!AcceptanceQueue::path(&root).exists());
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn receipts_are_unique_across_accepts_in_the_same_millisecond() {
    let root = temp_root("unique");
    let queue = AcceptanceQueue::new(&root);
    let a = queue.accept(None, "a", 1_000).expect("a");
    let b = queue.accept(None, "b", 1_000).expect("b");
    assert_ne!(a.id, b.id);
    assert_eq!(queue.pending().expect("pending").len(), 2);
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn the_journal_only_ever_grows_and_keeps_earlier_lines() {
    let root = temp_root("append");
    let queue = AcceptanceQueue::new(&root);
    queue.accept(Some("home"), "first", 1_000).expect("first");
    let after_first =
        fs::read_to_string(AcceptanceQueue::path(&root)).expect("read");

    queue.accept(Some("home"), "second", 2_000).expect("second");
    let after_second =
        fs::read_to_string(AcceptanceQueue::path(&root)).expect("read");

    // The earlier bytes are still there, unchanged, as a prefix. A snapshot
    // file could not make this promise.
    assert!(
        after_second.starts_with(&after_first),
        "appending must not rewrite history"
    );
    assert_eq!(after_second.lines().count(), 2);
    assert_eq!(queue.all().expect("all").len(), 2);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_truncated_final_line_is_tolerated() {
    let root = temp_root("truncated");
    let queue = AcceptanceQueue::new(&root);
    queue.accept(None, "complete", 1_000).expect("accept");
    let path = AcceptanceQueue::path(&root);
    let mut handle = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open");
    handle
        .write_all(br#"{"event":"accepted","id":"acc"#)
        .expect("partial write");
    drop(handle);

    // The torn tail is dropped; the complete record survives.
    let pending = queue.pending().expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].text, "complete");

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_torn_line_does_not_hide_receipts_appended_after_it() {
    let root = temp_root("torn-middle");
    let queue = AcceptanceQueue::new(&root);
    queue.accept(None, "first", 1_000).expect("first");
    let path = AcceptanceQueue::path(&root);
    // Simulate a crash in the middle of an append: a partial record with no
    // terminating newline.
    let mut handle = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open");
    handle
        .write_all(br#"{"event":"accepted","id":"torn"#)
        .expect("partial write");
    drop(handle);

    // The next accept must start on its own line; the torn line is skipped
    // instead of hiding everything after it.
    let second = queue.accept(None, "second", 2_000).expect("second");
    let pending = queue.pending().expect("pending");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].id, second.id);
    assert_eq!(pending[1].text, "second");

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn an_expired_lease_can_be_claimed_again() {
    let root = temp_root("lease-expiry");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "retry me", 1_000).expect("accept");
    assert!(
        queue
            .take(&accepted.id, "attempt-1", 1_000, 100)
            .expect("first")
            .is_some()
    );
    assert_eq!(
        queue.pending_at(1_050).expect("pending").len(),
        0,
        "the live lease must keep the receipt out of pending"
    );
    let second = queue
        .take(&accepted.id, "attempt-2", 1_101, 100)
        .expect("second");
    let second = second.expect("an expired lease is claimable");
    assert_eq!(
        second.lease.as_ref().map(|lease| lease.attempt_id.as_str()),
        Some("attempt-2")
    );
    assert_eq!(queue.pending_at(1_101).expect("pending").len(), 0);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_completed_receipt_never_returns_to_pending() {
    let root = temp_root("lease-complete");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "done", 1_000).expect("accept");
    assert!(
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .is_some()
    );
    assert!(
        queue
            .complete(&accepted.id, "attempt-1", 1_200)
            .expect("complete")
    );

    assert_eq!(queue.pending_at(1_000_000).expect("pending").len(), 0);
    assert!(
        queue
            .take(&accepted.id, "attempt-3", 1_000_000, 100)
            .expect("take")
            .is_none()
    );
    let all = queue.all().expect("all");
    assert_eq!(all[0].completed_at_ms, Some(1_200));

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_released_receipt_is_claimable_again() {
    let root = temp_root("lease-release");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "retry later", 1_000).expect("accept");
    assert!(
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .is_some()
    );
    assert!(
        queue
            .release(&accepted.id, "attempt-1", 1_200, 0)
            .expect("release")
    );
    assert_eq!(queue.pending_at(1_200).expect("pending").len(), 1);
    assert!(
        queue
            .take(&accepted.id, "attempt-2", 1_200, 100)
            .expect("take again")
            .is_some()
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_released_receipt_waits_for_its_backoff() {
    let root = temp_root("lease-backoff");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "slow retry", 1_000).expect("accept");
    assert!(
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .is_some()
    );
    assert!(
        queue
            .release(&accepted.id, "attempt-1", 1_100, 4_000)
            .expect("release")
    );

    assert_eq!(queue.pending_at(1_100).expect("pending").len(), 0);
    assert_eq!(queue.pending_at(5_099).expect("pending").len(), 0);
    assert_eq!(queue.pending_at(5_100).expect("pending").len(), 1);
    assert!(
        queue
            .take(&accepted.id, "attempt-2", 5_100, 100)
            .expect("take")
            .is_some()
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_given_up_receipt_is_never_claimable_again() {
    let root = temp_root("dead");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "poison", 1_000).expect("accept");
    queue
        .take(&accepted.id, "attempt-1", 1_000, 30_000)
        .expect("take")
        .expect("claimed");
    assert!(
        queue
            .give_up(&accepted.id, "attempt-1", 2_000)
            .expect("give up")
    );

    assert_eq!(queue.pending_at(10_000_000).expect("pending"), Vec::new());
    assert!(queue.all().expect("all")[0].dead);
    assert!(
        queue
            .take(&accepted.id, "attempt-2", 10_000_000, 30_000)
            .expect("take")
            .is_none()
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_stale_attempt_cannot_bury_a_live_retry() {
    let root = temp_root("dead-stale");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "retry me", 1_000).expect("accept");
    queue
        .take(&accepted.id, "attempt-1", 1_000, 30_000)
        .expect("take")
        .expect("claimed");
    queue
        .release(&accepted.id, "attempt-1", 1_100, 0)
        .expect("release");
    queue
        .take(&accepted.id, "attempt-2", 1_100, 30_000)
        .expect("take")
        .expect("claimed");

    assert!(
        !queue
            .give_up(&accepted.id, "attempt-1", 1_200)
            .expect("give up")
    );
    let receipt = &queue.all().expect("all")[0];
    assert!(!receipt.dead);
    assert!(receipt.lease.is_some(), "the live attempt keeps its lease");

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn only_the_lease_owner_can_complete_or_release() {
    let root = temp_root("lease-owner");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "owned", 1_000).expect("accept");
    assert!(
        queue
            .take(&accepted.id, "attempt-1", 1_000, 30_000)
            .expect("take")
            .is_some()
    );
    assert!(
        !queue
            .complete(&accepted.id, "attempt-2", 1_200)
            .expect("wrong owner")
    );
    assert!(
        !queue
            .release(&accepted.id, "attempt-2", 1_200, 0)
            .expect("wrong owner")
    );
    assert!(
        queue
            .complete(&accepted.id, "attempt-1", 1_200)
            .expect("owner")
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn two_threads_cannot_claim_the_same_receipt() {
    let root = temp_root("lease-race");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "one winner", 1_000).expect("accept");
    let first_queue = queue.clone();
    let first_id = accepted.id.clone();
    let first = std::thread::spawn(move || {
        first_queue.take(&first_id, "attempt-a", 1_000, 30_000)
    });
    let second_queue = queue;
    let second_id = accepted.id;
    let second = std::thread::spawn(move || {
        second_queue.take(&second_id, "attempt-b", 1_000, 30_000)
    });
    let first = first.join().expect("thread").expect("io");
    let second = second.join().expect("thread").expect("io");
    assert_ne!(first.is_some(), second.is_some());

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn an_old_taken_line_is_read_as_an_expired_lease() {
    let root = temp_root("lease-old-line");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "old line", 1_000).expect("accept");
    let path = AcceptanceQueue::path(&root);
    let mut handle = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open");
    let old = serde_json::json!({
        "event": "taken",
        "id": &accepted.id,
        "at_ms": 2_000
    });
    writeln!(handle, "{old}").expect("old line");
    drop(handle);

    assert_eq!(queue.pending_at(3_000).expect("pending").len(), 1);
    let claimed = queue
        .take(&accepted.id, "attempt-new", 3_000, 100)
        .expect("take");
    let claimed = claimed.expect("old taken line is retryable");
    assert_eq!(
        claimed
            .lease
            .as_ref()
            .map(|lease| lease.attempt_id.as_str()),
        Some("attempt-new")
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn binding_persists_session_and_submission_and_reconciles_after_lease_expiry() {
    let root = temp_root("bind-reconcile");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "ship deck", 1_000).expect("accept");
    assert_eq!(accepted.state_at(1_000), ReceiptState::Queued);

    let claimed = queue
        .take(&accepted.id, "attempt-1", 1_100, 500)
        .expect("take")
        .expect("claimed");
    assert_eq!(claimed.state_at(1_200), ReceiptState::Running);
    assert!(
        queue
            .bind(&accepted.id, "attempt-1", "s-99", "submission-abc", 1_150)
            .expect("bind")
    );

    let after_expiry = queue.get(&accepted.id).expect("get").expect("found");
    assert_eq!(after_expiry.target_session_id.as_deref(), Some("s-99"));
    assert_eq!(
        after_expiry.submission_id.as_deref(),
        Some("submission-abc")
    );
    assert_eq!(after_expiry.state_at(5_000), ReceiptState::Retrying);

    assert!(
        queue
            .complete_reconciled(&accepted.id, "attempt-1", 5_100)
            .expect("reconcile")
    );
    let settled = queue.get(&accepted.id).expect("get").expect("found");
    assert_eq!(settled.state_at(5_200), ReceiptState::Completed);
    assert_eq!(queue.pending_at(5_200).expect("pending").len(), 0);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn renew_lease_extends_active_claim_and_rejects_stale_owner() {
    let root = temp_root("renew-lease");
    let queue = AcceptanceQueue::new(&root);
    let accepted = queue.accept(None, "long work", 1_000).expect("accept");
    let claimed = queue
        .take(&accepted.id, "attempt-1", 1_000, 300)
        .expect("take")
        .expect("claimed");
    assert_eq!(claimed.state_at(1_200), ReceiptState::Running);

    assert!(
        !queue
            .renew_lease(&accepted.id, "wrong-attempt", 1_200, 300)
            .expect("wrong owner")
    );
    assert!(
        queue
            .renew_lease(&accepted.id, "attempt-1", 1_200, 500)
            .expect("renew")
    );
    // Past the original 1_300 expiry, the renewed 1_700 lease keeps it Running.
    let renewed = queue.get(&accepted.id).expect("get").expect("found");
    assert_eq!(renewed.state_at(1_500), ReceiptState::Running);
    assert_eq!(queue.pending_at(1_500).expect("pending").len(), 0);
    assert_eq!(renewed.state_at(1_700), ReceiptState::Retrying);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn cancel_prevents_future_claims_and_never_overrides_completed() {
    let root = temp_root("cancel");
    let queue = AcceptanceQueue::new(&root);
    let queued = queue.accept(None, "drop me", 1_000).expect("accept");
    assert!(queue.cancel(&queued.id, 1_050).expect("cancel"));
    assert!(!queue.cancel(&queued.id, 1_060).expect("idempotent cancel"));
    assert_eq!(
        queue
            .get(&queued.id)
            .expect("get")
            .expect("found")
            .state_at(1_100),
        ReceiptState::Cancelled
    );
    assert_eq!(queue.pending_at(1_100).expect("pending"), Vec::new());
    assert!(
        queue
            .take(&queued.id, "attempt-1", 1_100, 500)
            .expect("take")
            .is_none()
    );

    let done = queue.accept(None, "already done", 2_000).expect("accept");
    queue
        .take(&done.id, "attempt-1", 2_010, 500)
        .expect("take")
        .expect("claimed");
    assert!(queue.complete(&done.id, "attempt-1", 2_020).expect("done"));
    assert!(!queue.cancel(&done.id, 2_030).expect("cannot cancel done"));
    assert_eq!(
        queue
            .get(&done.id)
            .expect("get")
            .expect("found")
            .state_at(2_030),
        ReceiptState::Completed
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn kernel_review_corrupt_middle_terminal_event_never_reclaims_completed_receipt()
 {
    let root = temp_root("corrupt-middle-terminal");
    let queue = AcceptanceQueue::new(&root);
    let first = queue.accept(None, "first work", 1_000).expect("accept a");
    assert!(
        queue
            .take(&first.id, "attempt-1", 1_010, 100)
            .expect("take a")
            .is_some()
    );
    assert!(
        queue
            .complete(&first.id, "attempt-1", 1_020)
            .expect("complete a")
    );
    let second = queue.accept(None, "second work", 2_000).expect("accept b");

    // Corrupt the interior `completed` line for `first` while keeping the
    // subsequent valid `accepted` line for `second`.
    let path = AcceptanceQueue::path(&root);
    let raw = fs::read_to_string(&path).expect("read journal");
    let mut mutated: Vec<String> = raw.lines().map(str::to_owned).collect();
    let completed_idx = mutated
        .iter()
        .position(|line| line.contains("\"completed\""))
        .expect("completed line");
    mutated[completed_idx] = r#"{"event":"completed","corrupt"#.to_owned();
    fs::write(&path, format!("{}\n", mutated.join("\n")))
        .expect("rewrite journal");

    assert!(
        queue.has_interior_corruption().expect("corruption flag"),
        "interior corrupt line must be flagged"
    );
    let first_after = queue.get(&first.id).expect("get a").expect("found");
    assert!(first_after.quarantined);
    assert_eq!(first_after.state_at(9_999), ReceiptState::Quarantined);
    assert!(
        queue
            .take(&first.id, "attempt-2", 9_999, 1_000)
            .expect("take a after corruption")
            .is_none(),
        "corrupted terminal line must never allow silent re-claim"
    );
    // Subsequent valid receipts remain claimable and completable.
    let pending = queue.pending_at(9_999).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, second.id);
    assert!(
        queue
            .take(&second.id, "attempt-b", 9_999, 1_000)
            .expect("take b")
            .is_some()
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn visible_at_includes_backoff_running_dead_and_quarantined_in_order() {
    let root = temp_root("visible-states");
    let queue = AcceptanceQueue::new(&root);

    let r_done = queue.accept(None, "done", 1_000).expect("done");
    queue
        .take(&r_done.id, "a-done", 1_010, 500)
        .expect("take done")
        .expect("claimed");
    assert!(queue.complete(&r_done.id, "a-done", 1_020).expect("done"));

    let r_cancel = queue.accept(None, "cancel", 1_100).expect("cancel");
    assert!(queue.cancel(&r_cancel.id, 1_110).expect("cancel"));

    let r_backoff = queue.accept(None, "backoff", 1_200).expect("backoff");
    queue
        .take(&r_backoff.id, "a-back", 1_210, 500)
        .expect("take back")
        .expect("claimed");
    assert!(
        queue
            .release(&r_backoff.id, "a-back", 1_220, 60_000)
            .expect("release")
    );

    let r_dead = queue.accept(None, "dead", 1_300).expect("dead");
    queue
        .take(&r_dead.id, "a-dead", 1_310, 500)
        .expect("take dead")
        .expect("claimed");
    assert!(queue.give_up(&r_dead.id, "a-dead", 1_320).expect("dead"));

    let r_running = queue.accept(None, "running", 1_400).expect("running");
    queue
        .take(&r_running.id, "a-run", 1_410, 10_000)
        .expect("take run")
        .expect("claimed");

    let r_queued = queue.accept(None, "queued", 1_500).expect("queued");

    // During backoff (`now = 2_000`), `pending_at` hides backoff/dead/running,
    // while `visible_at` keeps every non-Completed, non-Cancelled item in
    // ascending `created_at_ms` order.
    let pending = queue.pending_at(2_000).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, r_queued.id);

    let visible = queue.visible_at(2_000).expect("visible");
    let summary: Vec<(String, ReceiptState)> =
        visible.into_iter().map(|(m, s)| (m.id, s)).collect();
    assert_eq!(
        summary,
        vec![
            (r_backoff.id, ReceiptState::Retrying),
            (r_dead.id, ReceiptState::Dead),
            (r_running.id, ReceiptState::Running),
            (r_queued.id, ReceiptState::Queued),
        ]
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn retry_dead_supersedes_original_and_is_idempotent_across_reopen() {
    let root = temp_root("retry-dead-supersede");
    let queue = AcceptanceQueue::new(&root);

    let dead = queue
        .accept(Some("sess-1"), "deliver deck", 1_000)
        .expect("accept");
    queue
        .take(&dead.id, "a1", 1_010, 500)
        .expect("take")
        .expect("claimed");
    assert!(queue.give_up(&dead.id, "a1", 1_020).expect("give_up"));

    let requeued = queue
        .retry_dead(&dead.id, 2_000)
        .expect("retry_dead")
        .expect("requeued");
    assert_ne!(requeued.id, dead.id);
    assert_eq!(requeued.session_hint.as_deref(), Some("sess-1"));
    assert_eq!(requeued.text, "deliver deck");

    // Second retry on the already-superseded dead id must be rejected.
    assert!(
        queue
            .retry_dead(&dead.id, 2_010)
            .expect("second retry")
            .is_none()
    );

    // Fresh handle after simulated restart only sees the new queued receipt.
    let reopened = AcceptanceQueue::new(&root);
    let old = reopened.get(&dead.id).expect("get old").expect("present");
    assert!(old.dead);
    assert!(old.cancelled);
    assert_eq!(old.state_at(2_050), ReceiptState::Cancelled);

    let visible = reopened.visible_at(2_050).expect("visible");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].0.id, requeued.id);
    assert_eq!(visible[0].1, ReceiptState::Queued);

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn torn_tail_repaired_in_place_never_quarantines_earlier_taken_receipt() {
    let root = temp_root("torn-tail-repair");
    let queue = AcceptanceQueue::new(&root);
    let first = queue.accept(None, "first", 1_000).expect("accept first");
    queue
        .take(&first.id, "att-1", 1_010, 5_000)
        .expect("take")
        .expect("claimed");
    // Append an incomplete non-newline crash tail.
    let path = AcceptanceQueue::path(&root);
    let mut handle = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open");
    handle
        .write_all(br#"{"event":"renewed","id":"torn"#)
        .expect("torn write");
    drop(handle);

    // Next append must truncate the torn tail instead of promoting it into
    // an interior corrupt line that permanently quarantines `first`.
    let second = queue.accept(None, "second", 2_000).expect("accept second");
    assert!(!queue.has_interior_corruption().expect("corrupt flag"));
    let first_after = queue.get(&first.id).expect("get").expect("present");
    assert!(
        !first_after.quarantined,
        "repaired torn tail must not quarantine healthy taken receipt"
    );
    assert_eq!(first_after.state_at(2_000), ReceiptState::Running);
    assert_eq!(
        queue
            .get(&second.id)
            .expect("get second")
            .expect("present")
            .state_at(2_000),
        ReceiptState::Queued
    );
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn release_uncommitted_leases_moves_dead_predecessor_running_to_retrying() {
    let root = temp_root("release-stale-leases");
    let queue = AcceptanceQueue::new(&root);
    let r = queue
        .accept(None, "crashed mid-turn", 1_000)
        .expect("accept");
    queue
        .take(&r.id, "att-dead-proc", 1_010, 30_000)
        .expect("take")
        .expect("claimed");
    assert_eq!(
        queue.get(&r.id).unwrap().unwrap().state_at(2_000),
        ReceiptState::Running
    );

    let released = queue
        .release_uncommitted_leases_except(2_000, &BTreeSet::new())
        .expect("release stale leases");
    assert_eq!(released, vec![r.id.clone()]);
    assert_eq!(
        queue.get(&r.id).unwrap().unwrap().state_at(2_000),
        ReceiptState::Retrying
    );
    assert_eq!(queue.pending_at(2_000).unwrap().len(), 1);

    // A receipt bound to session B whose stale session_hint points to
    // protected session A must still be released on cold boot.
    let r2 = queue
        .accept(Some("session-a"), "rebound to b", 2_100)
        .expect("accept r2");
    queue
        .take(&r2.id, "att-r2", 2_110, 30_000)
        .expect("take r2")
        .expect("claimed r2");
    queue
        .bind(&r2.id, "att-r2", "session-b", "sub-r2", 2_120)
        .expect("bind r2");
    let protected = BTreeSet::from(["session-a".to_owned()]);
    let released_r2 = queue
        .release_uncommitted_leases_except(2_200, &protected)
        .expect("release rebound r2");
    assert_eq!(released_r2, vec![r2.id]);
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn draft_target_survives_journal_replay_and_retry_dead_across_reopen() {
    let root = temp_root("draft-target");
    let queue = AcceptanceQueue::new(&root);
    let target = DraftTarget {
        origin: SessionOrigin::Project {
            project_id: Some("proj-alpha".into()),
        },
        workspace: Some(SessionWorkspace {
            root: PathBuf::from(r"C:\work\proj-alpha"),
        }),
        role: SessionRole::Router,
    };
    let accepted = queue
        .accept_with_target(
            None,
            "cold start draft prompt",
            false,
            Some(target.clone()),
            None,
            1_000,
        )
        .expect("accept draft");
    assert_eq!(accepted.draft_target.as_deref(), Some(&target));

    let reopened = AcceptanceQueue::new(&root);
    let replayed = reopened.get(&accepted.id).unwrap().unwrap();
    assert_eq!(replayed.draft_target.as_deref(), Some(&target));

    reopened
        .take(&accepted.id, "att-1", 1_100, 30_000)
        .unwrap()
        .unwrap();
    assert!(reopened.give_up(&accepted.id, "att-1", 1_200).unwrap());
    let retried = reopened
        .retry_dead(&accepted.id, 1_300)
        .unwrap()
        .expect("retry_dead preserves draft target");
    assert_eq!(retried.draft_target.as_deref(), Some(&target));

    let reopened_again = AcceptanceQueue::new(&root);
    let retried_replayed = reopened_again.get(&retried.id).unwrap().unwrap();
    assert_eq!(retried_replayed.draft_target.as_deref(), Some(&target));

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn tail_cache_hits_unchanged_and_incrementally_parses_appends_including_torn_tail()
 {
    let root = temp_root("tail-cache-incremental");
    let queue = AcceptanceQueue::new(&root);
    // Write enough entries so file exceeds 4096 bytes (exercises separate
    // head and tail boundary slices).
    let padding = "x".repeat(256);
    for i in 0..24_u64 {
        let _ = queue
            .accept(Some("sess"), &format!("msg-{i}-{padding}"), 1_000 + i)
            .expect("accept");
    }
    let path = AcceptanceQueue::path(&root);
    assert!(fs::metadata(&path).unwrap().len() > 4096);

    let _ = take_replay_stats();
    let initial = queue.all().expect("initial replay");
    assert_eq!(initial.len(), 24);
    assert_eq!(
        take_replay_stats(),
        ReplayStats {
            cache_hits: 0,
            tail_parses: 0,
            full_replays: 1
        }
    );

    // Unchanged second read must hit cache directly without full replay.
    let cached_read = queue.all().expect("cached replay");
    assert_eq!(cached_read, initial);
    assert_eq!(
        take_replay_stats(),
        ReplayStats {
            cache_hits: 1,
            tail_parses: 0,
            full_replays: 0
        }
    );

    // Append a torn trailing fragment without `\n`: parsed_len must NOT
    // advance across the partial fragment, and incremental tail read must
    // tolerate it cleanly.
    let raw_before_torn = fs::read(&path).expect("read before torn");
    {
        let mut handle = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open append");
        handle
            .write_all(br#"{"event":"accepted","id":"torn-"#)
            .expect("write torn");
    }
    let with_torn = queue.all().expect("replay with torn");
    assert_eq!(with_torn.len(), 24);
    assert_eq!(
        take_replay_stats(),
        ReplayStats {
            cache_hits: 0,
            tail_parses: 1,
            full_replays: 0
        }
    );

    // Complete the torn fragment externally into a valid `\n`-terminated
    // line; incremental tail parse must pick up ONLY from previous
    // `parsed_len` (`raw_before_torn.len()`).
    let completed_line = JournalEntry::Accepted {
        id: "ext-completed".into(),
        session_hint: Some("sess".into()),
        text: "completed externally".into(),
        at_ms: 2_000,
        strict_target: false,
        dispatch_request_id: None,
        draft_target: None,
    };
    let mut repaired_bytes = raw_before_torn;
    repaired_bytes
        .extend_from_slice(&serde_json::to_vec(&completed_line).unwrap());
    repaired_bytes.push(b'\n');
    fs::write(&path, &repaired_bytes).expect("write completed");

    let after_complete = queue.all().expect("replay after external line");
    assert_eq!(after_complete.len(), 25);
    assert_eq!(after_complete[24].id, "ext-completed");
    assert_eq!(
        take_replay_stats(),
        ReplayStats {
            cache_hits: 0,
            tail_parses: 1,
            full_replays: 0
        }
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn tail_cache_fails_closed_on_truncation_and_same_length_head_or_tail_rewrite()
{
    let root = temp_root("tail-cache-fail-closed");
    let queue = AcceptanceQueue::new(&root);
    let padding = "p".repeat(256);
    for i in 0..30_u64 {
        queue
            .accept(Some("home"), &format!("item-{i:02}-{padding}"), 1_000 + i)
            .expect("accept");
    }
    let path = AcceptanceQueue::path(&root);
    let original = fs::read(&path).expect("read original");
    assert!(original.len() > 8192);

    let _ = take_replay_stats();
    let first = queue.all().expect("prime cache");
    assert_eq!(first.len(), 30);
    assert_eq!(take_replay_stats().full_replays, 1);

    // 1) Same-length external rewrite inside head (first 4096 bytes) must
    //    fail closed via boundary mismatch and fall back to full replay.
    let mut head_mutated = original.clone();
    let needle = b"item-00-p";
    let head_pos = head_mutated
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("needle in head");
    assert!(head_pos < 4096);
    head_mutated[head_pos..head_pos + needle.len()]
        .copy_from_slice(b"item-00-Q");
    assert_eq!(head_mutated.len(), original.len());
    fs::write(&path, &head_mutated).expect("write head mutated");

    let after_head_rewrite = queue.all().expect("replay after head rewrite");
    assert_eq!(take_replay_stats().full_replays, 1);
    assert!(after_head_rewrite[0].text.starts_with("item-00-Q"));

    // 2) Same-length external rewrite inside tail (last 4096 bytes) must
    //    also fail closed and re-read from offset 0.
    let mut tail_mutated = head_mutated.clone();
    let tail_needle = b"item-29-p";
    let tail_pos = tail_mutated
        .windows(tail_needle.len())
        .position(|w| w == tail_needle)
        .expect("needle in tail");
    assert!(tail_pos + 4096 >= tail_mutated.len());
    tail_mutated[tail_pos..tail_pos + tail_needle.len()]
        .copy_from_slice(b"item-29-Z");
    assert_eq!(tail_mutated.len(), head_mutated.len());
    fs::write(&path, &tail_mutated).expect("write tail mutated");

    let after_tail_rewrite = queue.all().expect("replay after tail rewrite");
    assert_eq!(take_replay_stats().full_replays, 1);
    assert!(after_tail_rewrite[29].text.starts_with("item-29-Z"));

    // 3) External truncation (`len < parsed_len`) must invalidate and fall
    //    back to full offset-0 replay.
    let first_line_end = tail_mutated
        .iter()
        .position(|&b| b == b'\n')
        .expect("newline")
        + 1;
    fs::write(&path, &tail_mutated[..first_line_end])
        .expect("truncate externally");
    let after_truncate = queue.all().expect("replay after truncate");
    assert_eq!(take_replay_stats().full_replays, 1);
    assert_eq!(after_truncate.len(), 1);
    assert!(after_truncate[0].text.starts_with("item-00-Q"));

    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn tail_cache_stays_bounded_under_max_entries() {
    let mut roots = Vec::new();
    for i in 0..80_usize {
        let root = temp_root(&format!("cache-bound-{i}"));
        let queue = AcceptanceQueue::new(&root);
        queue.accept(None, "bound", 1_000).expect("accept");
        let _ = queue.all().expect("populate cache");
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
        fs::remove_dir_all(&root).expect("cleanup");
    }
}

#[test]
fn accept_never_compacts_while_terminal_transition_compacts_preserving_fidelity_across_reopen()
 {
    let root = temp_root("safe-compact");
    let queue = AcceptanceQueue::new(&root);

    // 1) Append 260 completed receipts -> total > 512 lines if completed,
    //    but first test that `accept` alone NEVER compacts even > 512 lines.
    for i in 0..520_u64 {
        let _ = queue
            .accept(Some("s-init"), &format!("queued-{i}"), 1_000 + i)
            .expect("accept");
    }
    let path = AcceptanceQueue::path(&root);
    let accept_only_bytes = fs::read_to_string(&path).expect("read");
    assert_eq!(
        accept_only_bytes.lines().count(),
        520,
        "accept() must never trigger compaction"
    );

    // 2) Now build a rich mixed-state journal in a second root:
    //    - 250 completed receipts (various attempts, bindings, releases)
    //    - 10 cancelled receipts
    //    - 2 dead receipts (one un-superseded, one superseded via retry_dead)
    //    - 3 non-terminal receipts (Queued, Running, Released-with-backoff)
    let rich_root = temp_root("safe-compact-rich");
    let rich = AcceptanceQueue::new(&rich_root);
    let target = DraftTarget {
        origin: SessionOrigin::Project {
            project_id: Some("proj-compact".into()),
        },
        workspace: Some(SessionWorkspace {
            root: PathBuf::from(r"C:\work\compact"),
        }),
        role: SessionRole::Worker,
    };

    let mut expected_kept_ids = BTreeSet::new();
    for i in 0..250_u64 {
        let r = rich
            .accept_with_target(
                Some("hint-term"),
                &format!("done-{i}"),
                false,
                Some(target.clone()),
                Some(&format!("req-{i}")),
                10_000 + i * 10,
            )
            .expect("accept done");
        rich.take(&r.id, "att-1", 10_001 + i * 10, 5_000)
            .expect("take")
            .expect("claimed");
        if i % 3 == 0 {
            // Multi-attempt with release + bind on attempt 2
            rich.release(&r.id, "att-1", 10_002 + i * 10, 100)
                .expect("release");
            rich.take(&r.id, "att-2", 10_200 + i * 10, 5_000)
                .expect("take 2")
                .expect("claimed 2");
            rich.bind(
                &r.id,
                "att-2",
                &format!("sess-{i}"),
                &format!("sub-{i}"),
                10_201 + i * 10,
            )
            .expect("bind");
            rich.complete(&r.id, "att-2", 10_205 + i * 10)
                .expect("complete");
        } else {
            rich.complete(&r.id, "att-1", 10_005 + i * 10)
                .expect("complete");
        }
    }
    for i in 0..10_u64 {
        let r = rich
            .accept(Some("hint-cancel"), &format!("cancel-{i}"), 20_000 + i)
            .expect("accept cancel");
        assert!(rich.cancel(&r.id, 20_500 + i).expect("cancel"));
        expected_kept_ids.insert(r.id);
    }

    // Dead (un-superseded)
    let dead_keep = rich
        .accept(Some("hint-dead"), "dead keep", 21_000)
        .expect("dead keep");
    rich.take(&dead_keep.id, "att-dead", 21_010, 5_000)
        .expect("take dead")
        .expect("claimed");
    assert!(
        rich.give_up(&dead_keep.id, "att-dead", 21_020)
            .expect("give_up")
    );
    expected_kept_ids.insert(dead_keep.id.clone());

    // Dead + Superseded (must stay dead & cancelled across compaction so
    // retry_dead remains idempotent)
    let dead_sup = rich
        .accept(Some("hint-sup"), "dead superseded", 21_100)
        .expect("dead sup");
    rich.take(&dead_sup.id, "att-sup", 21_110, 5_000)
        .expect("take sup")
        .expect("claimed");
    assert!(
        rich.give_up(&dead_sup.id, "att-sup", 21_120)
            .expect("give_up")
    );
    let retried_from_sup = rich
        .retry_dead(&dead_sup.id, 21_130)
        .expect("retry_dead")
        .expect("requeued");
    expected_kept_ids.insert(dead_sup.id.clone());
    expected_kept_ids.insert(retried_from_sup.id);

    // Non-terminal: Queued (strict_target = true), Running (bound + renewed),
    // Released (with backoff)
    let nt_queued = rich
        .accept_strict(
            "worker-strict",
            "queued non-term",
            Some("req-q"),
            22_000,
        )
        .expect("accept strict");
    expected_kept_ids.insert(nt_queued.id);

    let nt_running = rich
        .accept(Some("hint-run"), "running non-term", 22_100)
        .expect("accept run");
    rich.take(&nt_running.id, "att-run", 22_110, 5_000)
        .expect("take run")
        .expect("claimed");
    rich.bind(&nt_running.id, "att-run", "sess-run", "sub-run", 22_115)
        .expect("bind run");
    rich.renew_lease(&nt_running.id, "att-run", 22_120, 15_000)
        .expect("renew run");
    expected_kept_ids.insert(nt_running.id);

    let nt_released = rich
        .accept(Some("hint-rel"), "released non-term", 22_200)
        .expect("accept rel");
    rich.take(&nt_released.id, "att-rel", 22_210, 5_000)
        .expect("take rel")
        .expect("claimed");
    rich.release(&nt_released.id, "att-rel", 22_220, 3_000)
        .expect("release rel");
    expected_kept_ids.insert(nt_released.id);

    let before_snapshot = rich.all().expect("snapshot before compact");
    let rich_path = AcceptanceQueue::path(&rich_root);
    let lines_before = fs::read_to_string(&rich_path)
        .expect("read before")
        .lines()
        .count();
    assert!(lines_before > COMPACT_MIN_LINES);

    // Trigger terminal compaction on one more completion (note: all prior
    // completions already hit COMPACT_MIN_LINES once line 512 was crossed,
    // and we can also verify compact_with_thresholds directly).
    let final_done = rich
        .accept(Some("hint-final"), "final done", 23_000)
        .expect("final");
    rich.take(&final_done.id, "att-fin", 23_010, 5_000)
        .expect("take fin")
        .expect("claimed");
    assert!(
        rich.complete(&final_done.id, "att-fin", 23_020)
            .expect("complete fin")
    );
    expected_kept_ids.insert(final_done.id);

    let lines_after = fs::read_to_string(&rich_path)
        .expect("read after")
        .lines()
        .count();
    assert!(
        lines_after < lines_before,
        "terminal compaction must prune older terminal records ({lines_after} < {lines_before})"
    );

    // Reopen fresh handle (no cache reuse across handles if invalidated,
    // and full roundtrip verification).
    let _ = take_replay_stats();
    let reopened = AcceptanceQueue::new(&rich_root);
    let after_reopen = reopened.all().expect("replay after compact");
    assert_eq!(
        take_replay_stats().full_replays,
        1,
        "compaction must invalidate cache"
    );

    // Verify exact count: 200 newest terminal + 2 dead + 4 non-terminal
    // (`retried_from_sup`, `nt_queued`, `nt_running`, `nt_released`) = 206.
    assert_eq!(after_reopen.len(), 206);

    let before_by_id: HashMap<String, AcceptedMessage> = before_snapshot
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect();
    for kept in &after_reopen {
        if let Some(prev) = before_by_id.get(&kept.id) {
            assert_eq!(
                kept, prev,
                "compacted receipt {} must roundtrip 100% losslessly",
                kept.id
            );
        }
    }
    for id in &expected_kept_ids {
        assert!(
            after_reopen.iter().any(|r| &r.id == id),
            "expected kept receipt {id} missing after compaction"
        );
    }
    // `retry_dead` on superseded dead receipt must still be rejected!
    assert!(
        reopened
            .retry_dead(&dead_sup.id, 30_000)
            .expect("idempotent retry_dead")
            .is_none()
    );
    // Un-superseded dead receipt must still be retryable!
    assert!(
        reopened
            .retry_dead(&dead_keep.id, 30_001)
            .expect("retry_dead on kept dead")
            .is_some()
    );

    // Also test deterministic helper `compact_with_thresholds` does not
    // compact when below threshold.
    let small_root = temp_root("compact-threshold");
    let small = AcceptanceQueue::new(&small_root);
    for i in 0..5_u64 {
        let r = small.accept(None, &format!("s-{i}"), 1_000 + i).unwrap();
        small.take(&r.id, "a", 1_010 + i, 5_000).unwrap().unwrap();
        small.complete(&r.id, "a", 1_020 + i).unwrap();
    }
    assert!(
        !small
            .compact_with_thresholds(100, 1024 * 1024)
            .expect("below threshold")
    );

    fs::remove_dir_all(&root).expect("cleanup");
    fs::remove_dir_all(&rich_root).expect("cleanup");
    fs::remove_dir_all(&small_root).expect("cleanup");
}

#[test]
fn compaction_skips_corrupt_journals_preserving_quarantine_flag() {
    let root = temp_root("compact-corrupt-skip");
    let queue = AcceptanceQueue::new(&root);
    let first = queue.accept(None, "in-flight", 1_000).expect("first");
    queue
        .take(&first.id, "att-1", 1_010, 5_000)
        .expect("take")
        .expect("claimed");
    let path = AcceptanceQueue::path(&root);
    {
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, r#"{{"event":"broken""#).unwrap();
    }
    for i in 0..10_u64 {
        let r = queue.accept(None, &format!("ok-{i}"), 2_000 + i).unwrap();
        queue.take(&r.id, "a", 2_010 + i, 5_000).unwrap().unwrap();
        queue.complete(&r.id, "a", 2_020 + i).unwrap();
    }
    assert!(queue.has_interior_corruption().unwrap());
    assert!(
        !queue
            .compact_with_thresholds(1, 1)
            .expect("must skip corrupt journal")
    );
    assert!(queue.has_interior_corruption().unwrap());
    assert!(queue.get(&first.id).unwrap().unwrap().quarantined);
    fs::remove_dir_all(&root).expect("cleanup");
}
