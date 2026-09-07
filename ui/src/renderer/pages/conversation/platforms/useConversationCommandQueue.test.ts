import { describe, expect, test } from 'bun:test';
import {
  createQueuedCommandItem,
  getQueueItemDeliveryState,
  getQueueItemRecovery,
  isQueueItemDeliveryInFlight,
  normalizeQueueState,
  updateQueuedCommand,
  type ConversationCommandQueueItem,
} from './useConversationCommandQueue';

const item = (overrides: Partial<ConversationCommandQueueItem> = {}): ConversationCommandQueueItem => ({
  id: '01900000-0000-7000-8000-000000000001',
  input: 'send this',
  files: [],
  created_at: 1,
  ...overrides,
});

describe('conversation command queue durable delivery metadata', () => {
  test('normalizes legacy entries into a queued item with zero recovery attempts', () => {
    const normalized = normalizeQueueState({ items: [item()], isPaused: false });
    expect(normalized.items).toHaveLength(1);
    expect(getQueueItemDeliveryState(normalized.items[0])).toBe('queued');
    expect(getQueueItemRecovery(normalized.items[0])).toEqual({
      admission_attempts: 0,
      transport_attempts: 0,
    });
  });

  test('keeps dispatching and retrying items locked but allows waiting-for-turn recovery to be reconciled', () => {
    expect(isQueueItemDeliveryInFlight(item({ delivery_state: 'dispatching' }))).toBe(true);
    expect(isQueueItemDeliveryInFlight(item({ delivery_state: 'retrying' }))).toBe(true);
    expect(isQueueItemDeliveryInFlight(item({ delivery_state: 'waiting_for_turn' }))).toBe(false);
    expect(isQueueItemDeliveryInFlight(item({ delivery_state: 'paused' }))).toBe(false);
  });

  test('editing a queued item mints a new id and resets recovery metadata', () => {
    const original = item({
      recovery: { admission_attempts: 1, transport_attempts: 3 },
      delivery_state: 'paused',
      workspace_path: 'C:/original-workspace',
    });
    const [updated] = updateQueuedCommand([original], original.id, { input: 'edited' });
    expect(updated.input).toBe('edited');
    expect(updated.id).not.toBe(original.id);
    expect(updated.workspace_path).toBe('C:/original-workspace');
    expect(updated.created_at).toBeGreaterThanOrEqual(original.created_at);
    expect(getQueueItemRecovery(updated)).toEqual({
      admission_attempts: 0,
      transport_attempts: 0,
    });
    expect(getQueueItemDeliveryState(updated)).toBe('queued');
  });

  test('new queue items use an idempotency key and start in the queued phase', () => {
    const created = createQueuedCommandItem({
      input: 'new message',
      files: ['a', 'a'],
      workspace_path: 'C:/workspace',
    });
    expect(created.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(created.files).toEqual(['a']);
    expect(created.workspace_path).toBe('C:/workspace');
    expect(getQueueItemDeliveryState(created)).toBe('queued');
    expect(getQueueItemRecovery(created)).toEqual({
      admission_attempts: 0,
      transport_attempts: 0,
    });
  });

  test('keeps legacy queue entries valid while preserving new workspace snapshots', () => {
    const legacy = normalizeQueueState({ items: [item()] });
    expect(legacy.items[0].workspace_path).toBeUndefined();

    const current = normalizeQueueState({
      items: [item({ workspace_path: 'D:/project' })],
    });
    expect(current.items[0].workspace_path).toBe('D:/project');
  });
});
