import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

/**
 * Structural contract tests for the edit/resubmit pipeline. These assert on the
 * *ordering* of source-level operations (read from disk) rather than runtime
 * behavior — the behavioral race/resurrection coverage lives in
 * `editResubmitResurrection.test.ts`.
 *
 * The contract under test:
 *  - the old suffix is captured and the barrier armed BEFORE the request;
 *  - a read-only exact edit receipt observation decides whether reconciliation
 *    is allowed; a response or a paginated window never does that by itself;
 *  - SendBox clears edit state only after the typed resolution is terminal.
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
    const observe = nomiHandler.indexOf('editResubmitState.invoke({');
    const confirming = nomiHandler.indexOf("onPhaseChange?.('confirming', continueConfirmation)");
    const reconcile = nomiHandler.indexOf(
      'reconcileConfirmedEditMutation(initialDelivery ?? observation.delivery);'
    );
    const cleanup = nomiHandler.indexOf('clearSubmittedDraftAttachments();');
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
    expect(observe).toBeGreaterThan(invoke);
    expect(confirming).toBeGreaterThan(invoke);
    expect(reconcile).toBeGreaterThan(observe);
    expect(cleanup).toBeGreaterThan(nomiHandler.indexOf("recovery.kind === 'success'"));
    expect(purge).toBeGreaterThan(-1);
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
      'onEditResubmit(\n        targetId,\n        targetCreatedAt,\n        finalMessage,\n        operationId,'
    );
    const accepted = editSubmitBranch.indexOf('.then((resolution) => {', submit);
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

describe('authoritative edit outcome verification (P1-2)', () => {
  test('ambiguous failures use the exact receipt observation and never scan a history window', () => {
    // A transport or HTTP rejection cannot prove the backend did NOT execute.
    // Recovery must use the edit-specific receipt plus exact target/replacement
    // identities, and it must reuse the same operation key.
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    expect(nomiHandler).toContain('editResubmitState.invoke({');
    expect(nomiHandler).toContain('idempotency_key: operationId');
    expect(nomiHandler).toContain('resolveEditResubmitRecovery');
    expect(nomiHandler).not.toContain('verifyEditResubmitTarget');
    expect(nomiHandler).not.toContain('getConversationMessages');
    expect(nomiHandler).not.toContain('page_size: 200');
    expect(nomiHandler.indexOf('revokeBarrier(')).toBeGreaterThan(
      nomiHandler.indexOf("recovery.kind === 'safe_failure'")
    );
  });
});

describe('authoritative failure domains (P0-3)', () => {
  test('post-mutation failure keeps the barrier and returns a typed draft-preserving result', () => {
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    expect(nomiHandler).toContain("recovery.kind === 'mutated'");
    expect(nomiHandler).toContain("observation.delivery?.result_ok === false");
    expect(nomiHandler).toContain("return { kind: 'post_mutation_failure', error }");
    expect(nomiHandler).toContain("reason: 'edit-resubmit-reconcile'");
    expect(nomiHandler).toContain("reason: 'edit-resubmit-failed'");
    expect(nomiHandler.indexOf("reason: 'edit-resubmit-failed'")).toBeGreaterThan(
      nomiHandler.indexOf("recovery.kind === 'safe_failure'")
    );
  });
});

describe('edit confirmation lifecycle (P1)', () => {
  test('stops stale confirmation work after unmount or conversation switch', () => {
    const nomiHandler = nomiSendBoxSource.slice(
      nomiSendBoxSource.indexOf('const handleEditResubmit = useCallback('),
      nomiSendBoxSource.indexOf('// Steering injects into the turn')
    );
    expect(nomiSendBoxSource).toContain('confirmationWaitRef.current?.();');
    expect(nomiSendBoxSource).toContain('lifecycleGenerationRef.current !== lifecycleGeneration');
    expect(nomiHandler).toContain('ensureOperationLive();');
    expect(nomiHandler).toContain("recovery.kind === 'requires_reset'");
    expect(nomiHandler).not.toContain('ipcBridge.conversation.reset.invoke({ conversation_id });');
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
      'onEditResubmit(\n        targetId,\n        targetCreatedAt,\n        finalMessage,\n        operationId,'
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
