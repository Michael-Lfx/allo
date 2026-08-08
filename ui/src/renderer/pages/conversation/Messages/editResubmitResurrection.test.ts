import { afterEach, describe, expect, test } from 'bun:test';

import type { TMessage } from '@/common/chat/chatLib';
import type { ConversationId, MessageId } from '@/common/types/ids';

import {
  __resetConversationMessageCoordinators,
  ackConsumerReconciled,
  armBarrier,
  beginEditResubmitReconciliation,
  captureBarrier,
  captureReconciliationSnapshot,
  describeConversation,
  filterFetchedRows,
  getEpoch,
  purgeCurrentRows,
  retainConsumer,
  revokeBarrier,
} from './conversationMessageCoordinator';
import { applyFetchedMessages, mergeFetchedMessagesForConversation } from './hooks';

import {
  applyFetch,
  conversationId,
  errorTip,
  errorTipMessageId,
  ids,
  oldAssistantMessageId,
  oldSuffix,
  otherConversationId,
  targetMessageId,
  textMessage,
} from './conversationTestData';

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

afterEach(() => {
  __resetConversationMessageCoordinators();
});

describe('captureBarrier', () => {
  test('captures the durable + stream identity of the suffix from the target row', () => {
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100);
    expect(capture).not.toBeNull();
    // getFetchedMergeKey derives from msg_id (the durable identity), not the
    // renderer-local id, so the keys carry the MessageIds.
    expect([...capture!.mergeKeys].sort()).toEqual(
      [
        `text:${targetMessageId}`,
        `text:${oldAssistantMessageId}`,
        `tips:${errorTipMessageId}`,
      ].sort()
    );
    expect(capture!.serverIds.has(targetMessageId)).toBe(true);
    expect(capture!.serverIds.has(oldAssistantMessageId)).toBe(true);
    expect(capture!.serverIds.has(errorTipMessageId)).toBe(true);
    // Every fixture row has a msg_id, so there are no stream-only local ids here.
    expect(capture!.localIds.size).toBe(0);
  });

  test('returns null (fail closed) when the target is not found by durable identity', () => {
    expect(captureBarrier(oldSuffix(), 'missing-id' as MessageId, 100)).toBeNull();
  });
});

describe('edit-resubmit resurrection', () => {
  test('variant A: a stale pre-truncate snapshot cannot resurrect the old suffix', () => {
    // The capturing instance already dropped the suffix; a stale fetched page
    // still contains the pre-truncate rows.
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-a', capture);
    beginEditResubmitReconciliation(conversationId, 'op-a');

    const currentList = [textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId)];
    const staleSnapshot = oldSuffix(); // prefix + old suffix, read before truncate

    const result = applyFetch(currentList, staleSnapshot, conversationId, getEpoch(conversationId));

    expect(ids(result)).toEqual(['prefix']);
    expect(ids(result)).not.toContain('target');
    expect(ids(result)).not.toContain('old-assistant');
    expect(ids(result)).not.toContain('old-error-tip');

    ackConsumerReconciled(conversationId, consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });

  test('variant A documents the raw merge channel that the barrier must close', () => {
    // Without the barrier filter, the same stale snapshot would merge the old
    // suffix back in as streamingOnly rows. This pins the channel the fix targets.
    const currentList = [textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId)];
    const merged = mergeFetchedMessagesForConversation(currentList, oldSuffix(), conversationId);
    expect(ids(merged)).toContain('target');
    expect(ids(merged)).toContain('old-assistant');
    expect(ids(merged)).toContain('old-error-tip');
  });

  test('variant B: id swap to DB ids does not defeat mergeKey/serverId filtering', () => {
    // withFetchedCanonicalIdentity rewrites local `id` to the durable DB id
    // between capture and removal. A local-id-only removal would no-op; the
    // barrier must still drop these rows via mergeKey/serverId.
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-b', capture);
    beginEditResubmitReconciliation(conversationId, 'op-b');

    // Same durable identity, but local id has been swapped to the DB id form.
    const swappedList: TMessage[] = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
      { ...textMessage('target', 'right', 100, targetMessageId), id: `db:${targetMessageId}` },
      { ...textMessage('old-assistant', 'left', 101, oldAssistantMessageId), id: `db:${oldAssistantMessageId}` },
      { ...errorTip('old-error-tip', 102, errorTipMessageId), id: `db:${errorTipMessageId}` },
    ];

    const purged = purgeCurrentRows(swappedList, conversationId);
    expect(ids(purged)).toEqual(['prefix']);

    ackConsumerReconciled(conversationId, consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });
});

describe('out-of-order fetch resolution', () => {
  test('a late pre-truncate fetch is discarded by the epoch guard', async () => {
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-o', capture);

    // Fetch O starts before the edit resolves and reads the pre-truncate DB.
    const oSnapshot = deferred<TMessage[]>();
    const oCapturedEpoch = getEpoch(conversationId);

    // Edit succeeds mid-flight: epoch bumps, barrier flips to reconciling.
    beginEditResubmitReconciliation(conversationId, 'op-o');
    const reconciledEpoch = getEpoch(conversationId);
    expect(reconciledEpoch).toBeGreaterThan(oCapturedEpoch);

    // Fetch F starts after the edit and reads the post-truncate (clean) DB.
    const fSnapshot = deferred<TMessage[]>();
    const fCapturedEpoch = getEpoch(conversationId);

    // The capturing instance already optimistically purged its suffix.
    let list: TMessage[] = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
    ];

    // F resolves first: clean snapshot applies, consumer acks, barrier retires.
    fSnapshot.resolve([
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
    ]);
    list = applyFetch(list, await fSnapshot.promise, conversationId, fCapturedEpoch);
    ackConsumerReconciled(conversationId, consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);

    // O resolves last: its captured epoch is stale, so it is discarded entirely.
    oSnapshot.resolve(oldSuffix());
    list = applyFetch(list, await oSnapshot.promise, conversationId, oCapturedEpoch);
    expect(ids(list)).toEqual(['prefix']);
    // Barrier stays retired — the late stale fetch resurrected nothing.
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });
});

describe('successive edits', () => {
  test('two barriers coexist and one latest authoritative fetch acks both', () => {
    const { consumerId } = retainConsumer(conversationId);

    const firstCapture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-1', firstCapture);
    beginEditResubmitReconciliation(conversationId, 'op-1');
    const firstEpoch = getEpoch(conversationId);

    // A second edit immediately follows before the first reconciles. Its target
    // is the freshly inserted bubble; capture a small new suffix for it.
    const secondTarget = '019fa2b0-6dc2-75c1-9b50-2742e02df2c0' as MessageId;
    const secondList = [
      textMessage('new-bubble', 'right', 200, secondTarget),
      errorTip('new-error-tip', 201, '019fa2b0-6dc2-75c1-9b50-2742e02df2c1' as MessageId),
    ];
    const secondCapture = captureBarrier(secondList, secondTarget, 200)!;
    armBarrier(conversationId, 'op-2', secondCapture);
    beginEditResubmitReconciliation(conversationId, 'op-2');
    const secondEpoch = getEpoch(conversationId);
    expect(secondEpoch).toBeGreaterThan(firstEpoch);

    // Both barriers are reconciling with this consumer pending.
    const barriers = describeConversation(conversationId).barriers;
    expect(barriers.map((barrier) => barrier.operationId).sort()).toEqual(['op-1', 'op-2']);

    // A single authoritative fetch at the latest epoch acks both at once.
    ackConsumerReconciled(conversationId, consumerId, secondEpoch);
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });
});

describe('failure convergence', () => {
  test('revoke stops filtering and the authoritative snapshot restores the old rows', () => {
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-fail', capture);

    // While armed, a stale fetched page is filtered (suffix not yet reconciled).
    const filteredWhileArmed = filterFetchedRows(oldSuffix(), conversationId);
    expect(ids(filteredWhileArmed)).toEqual(['prefix']);

    // The backend rejects the edit: revoke the barrier, then the failed-refresh
    // fetch returns the un-truncated authoritative snapshot.
    revokeBarrier(conversationId, 'op-fail');
    const restored = applyFetchedMessages(
      [textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId)],
      oldSuffix(),
      captureReconciliationSnapshot(conversationId)
    );
    expect(ids(restored)).toEqual(['prefix', 'target', 'old-assistant', 'old-error-tip']);
    // No leak: the revoked barrier is gone, and acking a now-absent barrier is a no-op.
    expect(describeConversation(conversationId).barriers).toEqual([]);
    ackConsumerReconciled(conversationId, consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });

  test('a failed refresh does not clear the barrier; a later successful ack does', () => {
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-retry', capture);
    const epoch = beginEditResubmitReconciliation(conversationId, 'op-retry')!;

    // First refresh attempt fails (network) — loadMessages never acks.
    expect(describeConversation(conversationId).barriers.length).toBe(1);

    // Retry succeeds: the consumer applies the epoch and acks.
    ackConsumerReconciled(conversationId, consumerId, epoch);
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });
});

describe('multi-consumer reconciliation', () => {
  test('one consumer acking does not retire a barrier that another consumer still owes', () => {
    const a = retainConsumer(conversationId);
    const b = retainConsumer(conversationId);

    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-multi', capture);
    beginEditResubmitReconciliation(conversationId, 'op-multi');

    const barriers = () => describeConversation(conversationId).barriers;
    expect(barriers().length).toBe(1);
    expect(barriers()[0].pendingConsumers.length).toBe(2);

    // Consumer A applies the authoritative fetch first.
    ackConsumerReconciled(conversationId, a.consumerId, getEpoch(conversationId));
    expect(barriers().length).toBe(1); // B still owes → barrier kept
    expect(barriers()[0].pendingConsumers).toEqual([b.consumerId]);

    // Consumer B applies it next → barrier retires.
    ackConsumerReconciled(conversationId, b.consumerId, getEpoch(conversationId));
    expect(barriers()).toEqual([]);

    a.release();
    b.release();
  });

  test('a consumer mounting mid-reconciliation joins the pending set', () => {
    const a = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-late', capture);
    beginEditResubmitReconciliation(conversationId, 'op-late');

    // Late mount: the new consumer must also complete a post-success load.
    const b = retainConsumer(conversationId);
    expect(describeConversation(conversationId).barriers[0].pendingConsumers.sort()).toEqual(
      [a.consumerId, b.consumerId].sort()
    );

    ackConsumerReconciled(conversationId, a.consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers.length).toBe(1); // B still owes
    ackConsumerReconciled(conversationId, b.consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);

    a.release();
    b.release();
  });

  test('one consumer plain fetch does not cancel another conversation epoch', () => {
    // Plain fetches never touch the epoch, so two consumers coexist without one
    // invalidating the other's valid responses.
    const a = retainConsumer(conversationId);
    const before = getEpoch(conversationId);
    // Simulate a no-op plain refresh cycle.
    applyFetchedMessages(
      [textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId)],
      [textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId)],
      captureReconciliationSnapshot(conversationId)
    );
    expect(getEpoch(conversationId)).toBe(before);
    a.release();
  });
});

describe('consumer lifecycle / leak prevention', () => {
  test('unmounting the last consumer keeps an armed transaction alive until its callback lands', () => {
    const a = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-armed', capture);

    // The capturing instance navigates away mid-request.
    a.release();

    // Armed barrier + no consumers: coordinator retained (in-flight request).
    let state = describeConversation(conversationId);
    expect(state.consumerCount).toBe(0);
    expect(state.barriers.length).toBe(1);
    expect(state.barriers[0].phase).toBe('armed');

    // The request eventually succeeds — reconcile snapshots the (now empty)
    // consumer set, so the barrier is immediately deletable on ack/destroy.
    beginEditResubmitReconciliation(conversationId, 'op-armed');
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });

  test('reconciling barrier with no consumers does not leak', () => {
    const a = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-leak', capture);
    beginEditResubmitReconciliation(conversationId, 'op-leak');
    a.release();
    // Reconciling + no consumers → destroyed, no permanent module-level residue.
    expect(describeConversation(conversationId).barriers).toEqual([]);
    expect(getEpoch(conversationId)).toBe(0);
  });
});

describe('row identity coverage & isolation', () => {
  test('unrelated prefix rows are never filtered', () => {
    retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-iso', capture);
    beginEditResubmitReconciliation(conversationId, 'op-iso');

    const fetched = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
      textMessage('older', 'left', 50, '019fa2b0-6dc2-75c1-9b50-2742e02df250' as MessageId),
    ];
    const result = applyFetchedMessages([], fetched, captureReconciliationSnapshot(conversationId));
    expect(ids(result).sort()).toEqual(['older', 'prefix']);
  });

  test('stream-only suffix rows (no msg_id) are purged on the capturing instance', () => {
    retainConsumer(conversationId);
    const list: TMessage[] = [
      textMessage('target', 'right', 100, targetMessageId),
      { id: 'stream-only-tip', conversation_id: conversationId, type: 'tips', position: 'left', created_at: 101, content: { content: 'x', type: 'error' } },
    ];
    const capture = captureBarrier(list, targetMessageId, 100)!;
    expect(capture.localIds.has('stream-only-tip')).toBe(true);

    armBarrier(conversationId, 'op-stream', capture);
    beginEditResubmitReconciliation(conversationId, 'op-stream');
    const purged = purgeCurrentRows(list, conversationId);
    expect(ids(purged)).toEqual([]);
  });

  test('barriers are isolated per conversation', () => {
    retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-conv', capture);
    beginEditResubmitReconciliation(conversationId, 'op-conv');
    expect(describeConversation(conversationId).barriers.length).toBe(1);

    // A fetched page for a different conversation is untouched.
    const otherRows = oldSuffix().map((message) => ({
      ...message,
      conversation_id: otherConversationId,
    }));
    expect(ids(filterFetchedRows(otherRows, otherConversationId))).toEqual(
      ids(otherRows)
    );
  });

  test('the optimistic new bubble yields exactly one copy after the authoritative fetch', () => {
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-one', capture);
    beginEditResubmitReconciliation(conversationId, 'op-one');

    const newMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2d0' as MessageId;
    const optimistic = textMessage('new-bubble', 'right', 200, newMessageId);

    // The authoritative fetch returns the same new message persisted in the DB.
    const fetched = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
      { ...optimistic, id: `db:${newMessageId}` },
    ];
    const result = applyFetchedMessages([optimistic], fetched, captureReconciliationSnapshot(conversationId));
    const newUserRows = result.filter(
      (message) => message.message_id === newMessageId
    );
    expect(newUserRows.length).toBe(1);

    ackConsumerReconciled(conversationId, consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });
});

describe('deferred updater race (P0-1)', () => {
  test('an updater executing after the barrier retired still reconciles from its frozen snapshot', () => {
    // Models React's deferred functional updates: loadMessages enqueues
    // update(updater) and acks SYNCHRONOUSLY (hooks.ts); the updater runs
    // later. With two consumers the second ack retires the barrier BEFORE the
    // first updater executes. The frozen snapshot keeps the reconciliation
    // rules alive past the barrier's deletion; consulting the live coordinator
    // here would find nothing and resurrect the old suffix (RED-recorded
    // against the previous live-lookup API: the suffix stayed in the list).
    const a = retainConsumer(conversationId);
    const b = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-deferred', capture);
    beginEditResubmitReconciliation(conversationId, 'op-deferred');
    const epoch = getEpoch(conversationId);

    const pendingUpdaters: Array<(list: TMessage[]) => TMessage[]> = [];
    const cleanPage = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
    ];

    // B's authoritative refresh is accepted: capture the snapshot, enqueue the
    // updater, then ack synchronously (exactly the mergeIntoList ordering).
    const snapshotB = captureReconciliationSnapshot(conversationId);
    expect(snapshotB.purge).toBe(true);
    pendingUpdaters.push((list) => applyFetchedMessages(list, cleanPage, snapshotB));
    ackConsumerReconciled(conversationId, b.consumerId, epoch);
    expect(describeConversation(conversationId).barriers.length).toBe(1); // A still owes

    // A's authoritative refresh is accepted the same way → barrier retires.
    const snapshotA = captureReconciliationSnapshot(conversationId);
    pendingUpdaters.push((list) => applyFetchedMessages(list, cleanPage, snapshotA));
    ackConsumerReconciled(conversationId, a.consumerId, epoch);
    expect(describeConversation(conversationId).barriers).toEqual([]);

    // React commits only now: both deferred updaters execute AFTER the barrier
    // is gone, against a list that still holds the old suffix.
    let list: TMessage[] = oldSuffix();
    for (const updater of pendingUpdaters) list = updater(list);

    expect(ids(list)).toEqual(['prefix']);

    a.release();
    b.release();
  });

  test('an armed barrier captured in the snapshot still filters stale fetched rows after retirement', () => {
    // Fetch accepted while the barrier is still ARMED (edit in flight). The
    // edit then succeeds and every consumer acks — retiring the barrier before
    // the deferred updater runs. The frozen armed rules must still filter the
    // stale pre-truncate page (armed rules filter fetched rows but must NOT
    // purge the current list: the suffix is legitimate until success).
    const a = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-armed-snap', capture);

    const snapshot = captureReconciliationSnapshot(conversationId);
    expect(snapshot.purge).toBe(false);
    expect(snapshot.mergeKeys.size).toBeGreaterThan(0);

    beginEditResubmitReconciliation(conversationId, 'op-armed-snap');
    ackConsumerReconciled(conversationId, a.consumerId, getEpoch(conversationId));
    expect(describeConversation(conversationId).barriers).toEqual([]);

    // Deferred updater runs now: the stale page's suffix rows are filtered out
    // even though the barrier is gone.
    const result = applyFetchedMessages([], oldSuffix(), snapshot);
    expect(ids(result)).toEqual(['prefix']);

    a.release();
  });

  test('a mixed snapshot (reconciling + armed) purges only the reconciling suffix', () => {
    // op-1 reconciling, op-2 armed at capture time. V5 semantics: armed
    // barriers filter fetched rows but leave the current list alone — op-2's
    // suffix stays until its own success/failure.
    const a = retainConsumer(conversationId);
    const firstCapture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-mix-1', firstCapture);
    beginEditResubmitReconciliation(conversationId, 'op-mix-1');

    const secondTarget = '019fa2b0-6dc2-75c1-9b50-2742e02df2c0' as MessageId;
    const secondSuffix = [
      textMessage('new-bubble', 'right', 200, secondTarget),
      errorTip('new-error-tip', 201, '019fa2b0-6dc2-75c1-9b50-2742e02df2c1' as MessageId),
    ];
    const secondCapture = captureBarrier(secondSuffix, secondTarget, 200)!;
    armBarrier(conversationId, 'op-mix-2', secondCapture);

    const snapshot = captureReconciliationSnapshot(conversationId);
    expect(snapshot.purge).toBe(true);

    // Current list holds both suffixes; fetched page is stale for both.
    const currentList = [...oldSuffix(), ...secondSuffix];
    const fetched = [
      textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
      ...oldSuffix().slice(1),
      ...secondSuffix,
    ];
    const result = applyFetchedMessages(currentList, fetched, snapshot);

    // op-1's suffix purged from the current list; op-2's armed suffix kept
    // (current-list copy survives) but its stale fetched copies filtered.
    expect(ids(result).sort()).toEqual(['new-bubble', 'new-error-tip', 'prefix'].sort());

    a.release();
  });
});

describe('barrier monotonicity & fail-closed begin (P0-3 / P2-2)', () => {
  test('revokeBarrier refuses a reconciling barrier and keeps it alive for acks', () => {
    // P0-3: once the backend has accepted (reconciling), a local failure must
    // NEVER revoke the barrier — the transition is one-way armed→reconciling.
    const { consumerId } = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-mono', capture);
    const epoch = beginEditResubmitReconciliation(conversationId, 'op-mono');

    expect(revokeBarrier(conversationId, 'op-mono')).toBe(false);
    expect(describeConversation(conversationId).barriers.length).toBe(1);
    expect(describeConversation(conversationId).barriers[0].phase).toBe('reconciling');

    // The surviving barrier still retires through the normal ack path.
    ackConsumerReconciled(conversationId, consumerId, epoch!);
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });

  test('revokeBarrier deletes an armed barrier and reports true', () => {
    retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-armed-revoke', capture);

    expect(revokeBarrier(conversationId, 'op-armed-revoke')).toBe(true);
    expect(describeConversation(conversationId).barriers).toEqual([]);
  });

  test('revokeBarrier on an unknown operation is a no-op reporting false', () => {
    retainConsumer(conversationId);
    expect(revokeBarrier(conversationId, 'op-nope')).toBe(false);
  });

  test('beginEditResubmitReconciliation fails closed without bumping the epoch', () => {
    // P2-2: a missing barrier must NOT bump the epoch — an orphan bump would
    // discard every innocent in-flight fetch for no reason. The consumer keeps
    // the coordinator alive so the epoch is observable.
    retainConsumer(conversationId);
    expect(beginEditResubmitReconciliation(conversationId, 'op-unknown')).toBeUndefined();
    expect(getEpoch(conversationId)).toBe(0);

    // Also fail closed when the coordinator exists but the barrier was revoked.
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-gone', capture);
    revokeBarrier(conversationId, 'op-gone');
    expect(beginEditResubmitReconciliation(conversationId, 'op-gone')).toBeUndefined();
    expect(getEpoch(conversationId)).toBe(0);
  });
});
