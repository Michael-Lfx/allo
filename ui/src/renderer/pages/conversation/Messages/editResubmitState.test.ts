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
