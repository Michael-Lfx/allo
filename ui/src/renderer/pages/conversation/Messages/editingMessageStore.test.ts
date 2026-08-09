import { afterEach, describe, expect, test } from 'bun:test';

import type { ConversationId, MessageId } from '@/common/types/ids';
import {
  __resetEditingMessageStore,
  clearEditingMessage,
  clearEditingMessageByOperation,
  getEditingMessage,
  getEditingMessageSnapshot,
  setEditingMessage,
  subscribeEditingMessage,
  updateEditingMessage,
  updateEditingMessageByOperation,
} from '@/renderer/pages/conversation/Messages/editingMessageStore';

const CID = 'conv-1' as ConversationId;
const CID2 = 'conv-2' as ConversationId;
const MID = (id: string): MessageId => id as MessageId;

/**
 * editingMessageStore 势在必行的命令式 API + 订阅语义（owner 护栏、响应式）。
 * Imperative API + subscription semantics (owner guard, reactivity) of the
 * editingMessageStore. The useEditingMessage hook only wraps useSyncExternalStore
 * over these primitives, so covering them covers the contract.
 */
describe('editingMessageStore', () => {
  afterEach(() => {
    __resetEditingMessageStore();
  });

  test('setEditingMessage writes per-conversation state', () => {
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    expect(getEditingMessage(CID)).toEqual({ ownerId: 'A', msgId: MID('m1'), pending: false });
    expect(getEditingMessage(CID2)).toBeUndefined();
  });

  test('setEditingMessage overwrites a prior entry for the same conversation', () => {
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m2'), pending: true, operationId: 'op' });
    expect(getEditingMessage(CID)).toEqual({ ownerId: 'A', msgId: MID('m2'), pending: true, operationId: 'op' });
  });

  test('updateEditingMessage applies a patch only when the owner matches', () => {
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    // Non-owner update is ignored.
    updateEditingMessage(CID, 'B', { pending: true });
    expect(getEditingMessage(CID)?.pending).toBe(false);
    // Owner update applies.
    updateEditingMessage(CID, 'A', { pending: true, operationId: 'op1' });
    expect(getEditingMessage(CID)).toEqual({
      ownerId: 'A',
      msgId: MID('m1'),
      pending: true,
      operationId: 'op1',
    });
  });

  test('a remounted owner can update and clear only the same durable operation', () => {
    setEditingMessage(CID, {
      ownerId: 'old-owner',
      msgId: MID('m1'),
      pending: true,
      phase: 'confirming',
      operationId: 'op-a',
    });
    updateEditingMessageByOperation(CID, 'op-b', { phase: 'editing' });
    expect(getEditingMessage(CID)?.phase).toBe('confirming');
    updateEditingMessageByOperation(CID, 'op-a', { continueConfirmation: () => undefined });
    expect(typeof getEditingMessage(CID)?.continueConfirmation).toBe('function');
    clearEditingMessageByOperation(CID, 'op-b');
    expect(getEditingMessage(CID)).toBeDefined();
    clearEditingMessageByOperation(CID, 'op-a');
    expect(getEditingMessage(CID)).toBeUndefined();
  });

  test('updateEditingMessage on a missing conversation is a no-op', () => {
    updateEditingMessage(CID, 'A', { pending: true });
    expect(getEditingMessage(CID)).toBeUndefined();
  });

  test('clearEditingMessage only clears when the owner matches (dual-instance guard)', () => {
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    // A second SendBox instance (B) unmounting must not clear A's state.
    clearEditingMessage(CID, 'B');
    expect(getEditingMessage(CID)).toBeDefined();
    // The owning instance clears its own state.
    clearEditingMessage(CID, 'A');
    expect(getEditingMessage(CID)).toBeUndefined();
  });

  test('clearEditingMessage on a missing conversation is a no-op', () => {
    clearEditingMessage(CID, 'A');
    expect(getEditingMessage(CID)).toBeUndefined();
  });

  test('writes notify subscribers (snapshot reference changes); guard-miss no-ops do not', () => {
    let notified = 0;
    const initialSnapshot = getEditingMessageSnapshot();
    const unsub = subscribeEditingMessage(() => {
      notified += 1;
    });

    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    expect(notified).toBe(1);
    // The snapshot reference changes on write, exactly what useSyncExternalStore
    // needs to re-render subscribed components.
    const snapshotAfterWrite = getEditingMessageSnapshot();
    expect(snapshotAfterWrite).not.toBe(initialSnapshot);
    expect(snapshotAfterWrite.get(CID)).toEqual({ ownerId: 'A', msgId: MID('m1'), pending: false });

    // Guard miss: update by a non-owner mutates nothing, so no notification.
    updateEditingMessage(CID, 'B', { pending: true });
    expect(notified).toBe(1);
    expect(getEditingMessageSnapshot()).toBe(snapshotAfterWrite);
    expect(getEditingMessage(CID)?.pending).toBe(false);

    // Owner update notifies and refreshes the snapshot.
    updateEditingMessage(CID, 'A', { pending: true, operationId: 'op' });
    expect(notified).toBe(2);
    expect(getEditingMessageSnapshot().get(CID)?.pending).toBe(true);

    unsub();
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m2'), pending: false });
    expect(notified).toBe(2);
  });

  test('per-conversation isolation', () => {
    setEditingMessage(CID, { ownerId: 'A', msgId: MID('m1'), pending: false });
    setEditingMessage(CID2, { ownerId: 'A', msgId: MID('m9'), pending: true, operationId: 'op' });
    expect(getEditingMessage(CID)?.msgId).toBe('m1');
    expect(getEditingMessage(CID2)?.msgId).toBe('m9');
    clearEditingMessage(CID, 'A');
    expect(getEditingMessage(CID)).toBeUndefined();
    expect(getEditingMessage(CID2)).toBeDefined();
  });
});
