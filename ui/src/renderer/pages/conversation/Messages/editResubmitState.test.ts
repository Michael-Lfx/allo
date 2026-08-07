import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

/**
 * Structural contract tests for the edit/resubmit pipeline. These assert on the
 * *ordering* of source-level operations (read from disk) rather than runtime
 * behavior — the behavioral race/resurrection coverage lives in
 * `editResubmitResurrection.test.ts`.
 *
 * The contract under test:
 *  - the old suffix is captured and the barrier armed BEFORE the request, and
 *    the suffix/attachments are only dropped AFTER backend acceptance;
 *  - SendBox clears edit state + input only inside the post-acceptance `.then`
 *    (this portion is enforced by `editResubmitState` on main and turns green
 *    once the SendBox timing fix lands).
 */

const sendBoxSource = readFileSync(
  new URL('../../../components/chat/SendBox/index.tsx', import.meta.url),
  'utf8'
);
const nomiSendBoxSource = readFileSync(
  new URL('../platforms/nomi/NomiSendBox.tsx', import.meta.url),
  'utf8'
);

describe('edit/resubmit pipeline structure', () => {
  test('NomiSendBox captures + arms before the request and tears down only after acceptance', () => {
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    const capture = nomiHandler.indexOf('captureBarrier(');
    const arm = nomiHandler.indexOf('armBarrier(conversation_id, operationId, capture);');
    const invoke = nomiHandler.indexOf('editResubmit.invoke({');
    const reconcile = nomiHandler.indexOf('beginEditResubmitReconciliation(conversation_id, operationId);');
    const purge = nomiHandler.indexOf('purgeCurrentRows(list, conversation_id)');
    // 附件集合差：提交时捕获已提交附件路径快照，成功后仅精确移除已提交项。
    // Attachment set-difference: snapshot the submitted attachment paths at submit,
    // then remove only those from the current selection after acceptance.
    const submittedAttachmentSnapshot = nomiHandler.indexOf(
      'const submittedAttachmentIds = new Set(filesToSend);'
    );
    const removeSubmitted = nomiHandler.indexOf('removeSubmittedAttachments(', invoke);
    const fullClear = nomiHandler.indexOf('clearFiles();', invoke);
    const failedRefresh = nomiHandler.indexOf("'edit-resubmit-failed'");

    // Durable capture, then arm, then the request.
    expect(capture).toBeGreaterThan(-1);
    expect(arm).toBeGreaterThan(capture);
    expect(invoke).toBeGreaterThan(arm);
    expect(submittedAttachmentSnapshot).toBeGreaterThan(-1);
    expect(invoke).toBeGreaterThan(submittedAttachmentSnapshot);
    // Post-acceptance only: reconcile flips the barrier, the suffix purge runs,
    // and the submitted attachments are removed via set-difference — all after invoke.
    expect(reconcile).toBeGreaterThan(invoke);
    expect(purge).toBeGreaterThan(reconcile);
    expect(removeSubmitted).toBeGreaterThan(invoke);
    // The full-clear helper must NOT appear in the edit-resubmit path (would wipe
    // attachments added mid-flight).
    expect(fullClear).toBe(-1);
    // Failure path revokes the barrier and emits a failed refresh.
    expect(failedRefresh).toBeGreaterThan(-1);
  });

  test('SendBox clears edit state and the old suffix only after backend acceptance', () => {
    const editSubmitBranch = sendBoxSource.slice(
      sendBoxSource.indexOf('if (editingMsgId && onEditResubmit) {'),
      sendBoxSource.indexOf('// Cancel any pending warmup:')
    );
    const submit = editSubmitBranch.indexOf(
      'onEditResubmit(targetId, targetCreatedAt, finalMessage)'
    );
    const accepted = editSubmitBranch.indexOf('.then(() => {', submit);
    const exitEditMode = editSubmitBranch.indexOf('setEditingMsgId(null);', submit);
    const clearInput = editSubmitBranch.indexOf("setInput('');", submit);

    expect(submit).toBeGreaterThan(-1);
    expect(accepted).toBeGreaterThan(submit);
    expect(exitEditMode).toBeGreaterThan(accepted);
    expect(clearInput).toBeGreaterThan(accepted);
  });

  test('retry path restores the retried text only when the input revision is unchanged', () => {
    // The error-popup retry also goes through onEditResubmit; its failure must
    // never overwrite what the user typed mid-flight (C2 inputRevision guard).
    const retryBranch = sendBoxSource.slice(
      sendBoxSource.indexOf("'sendbox.retry'"),
      sendBoxSource.indexOf('// Bump the input revision on every composer change')
    );
    const revisionSnapshot = retryBranch.indexOf(
      'const submittedInputRevision = inputRevisionRef.current;',
      retryBranch.indexOf("'sendbox.retry'")
    );
    const setLoading = retryBranch.indexOf('setIsLoading(true);', retryBranch.indexOf("'sendbox.retry'"));
    const restoreInput = retryBranch.indexOf('setInput(content);', retryBranch.indexOf("'sendbox.retry'"));
    const guard = retryBranch.indexOf(
      'inputRevisionRef.current === submittedInputRevision',
      retryBranch.indexOf("'sendbox.retry'")
    );

    expect(revisionSnapshot).toBeGreaterThan(-1);
    expect(setLoading).toBeGreaterThan(-1);
    // Snapshot before the request starts, guard before any restore.
    expect(setLoading).toBeGreaterThan(revisionSnapshot);
    expect(restoreInput).toBeGreaterThan(-1);
    expect(guard).toBeGreaterThan(-1);
    expect(restoreInput).toBeGreaterThan(guard);
  });
});

describe('retry operation mutex (P1-1)', () => {
  test('the retry branch rejects re-entry and gates its callbacks on an operation token', () => {
    // The error-popup retry shares the composer with the edit path: without its
    // own mutex, two retry events in the same tick (or a retry racing an edit
    // submit) would double-send and double-restore the input.
    expect(
      sendBoxSource.includes('const activeRetryOperationRef = useRef<string | null>(null);')
    ).toBe(true);

    const retryBranch = sendBoxSource.slice(
      sendBoxSource.indexOf("'sendbox.retry'"),
      sendBoxSource.indexOf('// Bump the input revision on every composer change')
    );
    const admission = retryBranch.indexOf('if (activeRetryOperationRef.current !== null) return;');
    const stamp = retryBranch.indexOf('activeRetryOperationRef.current = retryOperationId;');
    const setLoading = retryBranch.indexOf('setIsLoading(true);');
    const catchGuard = retryBranch.indexOf(
      'activeRetryOperationRef.current === retryOperationId &&'
    );
    const restoreInput = retryBranch.indexOf('setInput(content);');
    const finallyIndex = retryBranch.indexOf('} finally {');
    const release = retryBranch.indexOf('activeRetryOperationRef.current = null;', finallyIndex);
    const clearLoading = retryBranch.indexOf('setIsLoading(false);', finallyIndex);

    // Admission before any work; the token is stamped synchronously before the
    // request starts.
    expect(admission).toBeGreaterThan(-1);
    expect(stamp).toBeGreaterThan(admission);
    expect(setLoading).toBeGreaterThan(stamp);
    // Failure restore requires BOTH the token and the untouched input revision.
    expect(catchGuard).toBeGreaterThan(-1);
    expect(restoreInput).toBeGreaterThan(catchGuard);
    // finally releases the mutex before lowering the loading flag.
    expect(finallyIndex).toBeGreaterThan(-1);
    expect(release).toBeGreaterThan(finallyIndex);
    expect(clearLoading).toBeGreaterThan(release);
  });
});

describe('ambiguous transport outcome verification (P1-2)', () => {
  test('network/timeout failures consult the authoritative DB before revoking', () => {
    // A 'network'/'timeout' rejection cannot prove the backend did NOT execute
    // (connection reset / response lost after a durable truncate). Revoking
    // straight away would show a false "resend failed" toast for a message that
    // was actually sent. The transport catch must first check whether the
    // target message is gone (truncate durable ⇒ treat as success).
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    const firstTry = nomiHandler.indexOf('try {');
    const secondTry = nomiHandler.indexOf('try {', firstTry + 1);
    const transportDomain = nomiHandler.slice(firstTry, secondTry);

    const kindCheck = transportDomain.indexOf('isBackendRequestError(error)');
    const verify = transportDomain.indexOf('verifyEditResubmitTarget(');
    const revoke = transportDomain.indexOf('revokeBarrier(');

    expect(kindCheck).toBeGreaterThan(-1);
    expect(verify).toBeGreaterThan(kindCheck);
    // Verification runs BEFORE any revoke — only a proven failure revokes.
    expect(revoke).toBeGreaterThan(verify);
    // The true-failure path still emits the failed refresh.
    expect(transportDomain).toContain("'edit-resubmit-failed'");

    // The verifier itself reads the newest DB window (the edit target is always
    // inside the visible tail) and treats its own failure as "not proven".
    const verifierStart = nomiSendBoxSource.indexOf('verifyEditResubmitTarget = async');
    expect(verifierStart).toBeGreaterThan(-1);
    const verifier = nomiSendBoxSource.slice(verifierStart, verifierStart + 900);
    expect(verifier).toContain('getConversationMessages');
    expect(verifier).toContain('page_size: 200');
  });
});

describe('dual failure domains (P0-3)', () => {
  test('NomiSendBox splits transport failure from post-acceptance failure; only the former revokes', () => {
    // P0-3: after the backend's 202 the transcript is already truncated — a
    // local exception in the post-acceptance path must NEVER revoke the
    // (already reconciling) barrier. Revoke belongs exclusively to the
    // transport-failure domain.
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    const invoke = nomiHandler.indexOf('editResubmit.invoke({');
    const firstTry = nomiHandler.indexOf('try {');
    const secondTry = nomiHandler.indexOf('try {', firstTry + 1);

    // Two try blocks: the transport await sits in the first, the post-accept
    // reconciliation work in the second.
    expect(firstTry).toBeGreaterThan(-1);
    expect(secondTry).toBeGreaterThan(invoke);

    // Every revokeBarrier call lives in the transport domain (before the
    // second try); the post-accept domain contains none.
    let revokeAt = nomiHandler.indexOf('revokeBarrier(');
    while (revokeAt !== -1) {
      expect(revokeAt).toBeLessThan(secondTry);
      revokeAt = nomiHandler.indexOf('revokeBarrier(', revokeAt + 1);
    }
    expect(nomiHandler.indexOf('revokeBarrier(')).toBeGreaterThan(-1);

    // The post-accept domain still converges via a reconcile refresh, never a
    // failed refresh.
    const postAccept = nomiHandler.slice(secondTry);
    expect(postAccept).toContain("'edit-resubmit-reconcile'");
    expect(postAccept).not.toContain("'edit-resubmit-failed'");
  });
});

describe('edit submit mutex (P0-2 / P2-3)', () => {
  test('the edit branch rejects a second same-tick submit via the operation ref', () => {
    // isLoading is React state — it cannot block two submits in the same render
    // tick. The synchronously-updated activeEditOperationRef is the real mutex:
    // admission must reject when a resubmit is already in flight, and the
    // promise chain must release the ref in .finally (after .then/.catch).
    const editSubmitBranch = sendBoxSource.slice(
      sendBoxSource.indexOf('if (editingMsgId && onEditResubmit) {'),
      sendBoxSource.indexOf('// Cancel any pending warmup:')
    );
    const submit = editSubmitBranch.indexOf(
      'onEditResubmit(targetId, targetCreatedAt, finalMessage)'
    );
    const mutexAdmission = editSubmitBranch.indexOf(
      'if (activeEditOperationRef.current !== null) return;'
    );
    const stamp = editSubmitBranch.indexOf('activeEditOperationRef.current = operationId;');

    expect(submit).toBeGreaterThan(-1);
    expect(mutexAdmission).toBeGreaterThan(-1);
    // Admission guard runs before the request is stamped and sent.
    expect(mutexAdmission).toBeLessThan(stamp);
    expect(stamp).toBeLessThan(submit);

    // The ref is released in .finally, AFTER the .then/.catch outcome logic
    // (their isCurrentOperation() reads must still see the token).
    const finallyIndex = editSubmitBranch.indexOf('.finally(() => {', submit);
    const release = editSubmitBranch.indexOf('activeEditOperationRef.current = null;', finallyIndex);
    const clearLoading = editSubmitBranch.indexOf('setIsLoading(false);', finallyIndex);
    expect(finallyIndex).toBeGreaterThan(submit);
    expect(release).toBeGreaterThan(finallyIndex);
    expect(clearLoading).toBeGreaterThan(release);

    // The token guard still gates the .then/.catch side effects.
    expect(editSubmitBranch.indexOf('isCurrentOperation()', submit)).toBeGreaterThan(-1);
  });

  test('the sendbox.edit listener ignores a second edit while a resubmit is in flight', () => {
    // P2-3: entering edit mode mid-resubmit would let the user rewrite the
    // composer under an in-flight destructive request.
    const editListener = sendBoxSource.slice(
      sendBoxSource.indexOf("'sendbox.edit'"),
      sendBoxSource.indexOf("'sendbox.retry'")
    );
    const capability = editListener.indexOf('if (!onEditResubmit) return;');
    const inFlightGuard = editListener.indexOf(
      'if (isLoading || activeEditOperationRef.current !== null) return;'
    );
    const enterEdit = editListener.indexOf('setEditingMsgId(payload.msgId);');

    expect(capability).toBeGreaterThan(-1);
    expect(inFlightGuard).toBeGreaterThan(capability);
    expect(enterEdit).toBeGreaterThan(inFlightGuard);
  });
});
