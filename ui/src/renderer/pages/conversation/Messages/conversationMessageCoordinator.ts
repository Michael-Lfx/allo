import type { TMessage } from '@/common/chat/chatLib';
import type { ConversationId, MessageId } from '@/common/types/ids';

import { getFetchedMergeKey } from './messageRowKeys';

/**
 * Conversation-scoped message coordinator.
 *
 * The edit-resubmit flow truncates the backend transcript (target user message
 * + every later row, including persisted error tips) before its HTTP 202
 * returns. But several refetch triggers can be in flight during that window —
 * `conversation.turn.settled` (retry-forever poller), `turnCompleted`,
 * `reconnected`. A fetch that read the DB *before* the truncate can merge
 * *after* the frontend has already removed the old suffix, resurrecting the
 * old user message and error tip permanently (they then become streamingOnly
 * and never drop on their own). Variant B: `withFetchedCanonicalIdentity`
 * rewrites local row ids to DB ids between snapshot and removal, so a
 * local-id-based removal becomes a no-op.
 *
 * This coordinator closes that window with two layers of versioning plus
 * per-operation barriers:
 *
 *  - conversation `epoch` bumps ONLY on edit-resubmit success. A fetch that
 *    captured a stale epoch is discarded wholesale before its merge, so a
 *    pre-truncate snapshot can never be applied after the truncate commits.
 *    Plain fetches never touch the epoch, so one consumer's refresh cannot
 *    cancel another consumer's valid fetch (the flaw a global sequence would
 *    reintroduce).
 *  - instance-level `newestLoadSequenceRef` (in hooks.ts) is left intact for
 *    single-instance last-writer-wins ordering.
 *  - each edit-resubmit operation arms a barrier before the request. The
 *    barrier filters stale rows out of fetched pages while `armed`, and also
 *    purges each consumer's current list once `reconciling`. Consumers ack
 *    individually; a barrier is deleted only once every consumer that was
 *    active at success time has applied a post-success authoritative fetch.
 *
 * The module is pure TypeScript (no React, no emitter) so the race scenarios
 * are unit-testable with deferred Promises.
 */

export type ConsumerId = string;

/** Immutable filter sets captured from the old suffix at submit time. */
export interface EditResubmitBarrierCapture {
  /** `${type}:${msg_id}` for each durable suffix row. */
  mergeKeys: ReadonlySet<string>;
  /** Durable `message_id` for each suffix row that has one. */
  serverIds: ReadonlySet<string>;
  /** Renderer-local `id` of each *stream-only* suffix row (no `msg_id`).
   * Only the capturing instance can match these; cross-instance convergence
   * is promised solely via mergeKeys/serverIds. */
  localIds: ReadonlySet<string>;
  /** Target row's `created_at` — observability/fallback only, never a filter. */
  cutoffCreatedAt: number;
}

export interface EditResubmitBarrier {
  operationId: string;
  mergeKeys: ReadonlySet<string>;
  serverIds: ReadonlySet<string>;
  localIds: ReadonlySet<string>;
  cutoffCreatedAt: number;
  phase: 'armed' | 'reconciling';
  /** Conversation epoch stamped onto this barrier at success time. Only a
   * replace fetch whose applied epoch is `>= successEpoch` confirms this
   * consumer has read the post-truncate authoritative DB. */
  successEpoch?: number;
  pendingConsumers: Set<ConsumerId>;
}

interface ConversationMessageCoordinator {
  consumers: Set<ConsumerId>;
  epoch: number;
  barriers: Map<string, EditResubmitBarrier>;
}

const coordinators = new Map<ConversationId, ConversationMessageCoordinator>();
let nextConsumerSeq = 1;

const createCoordinator = (): ConversationMessageCoordinator => ({
  consumers: new Set(),
  epoch: 0,
  barriers: new Map(),
});

const getOrCreate = (conversationId: ConversationId): ConversationMessageCoordinator => {
  let coordinator = coordinators.get(conversationId);
  if (!coordinator) {
    coordinator = createCoordinator();
    coordinators.set(conversationId, coordinator);
  }
  return coordinator;
};

export const getEpoch = (conversationId: ConversationId): number =>
  coordinators.get(conversationId)?.epoch ?? 0;

/**
 * Capture the durable/stream identity of the old suffix starting at the target
 * user message. Returns `null` when the target cannot be located by durable
 * identity — callers MUST treat that as fail-closed (do not arm, do not send,
 * surface a generic error). The timestamp is stored for observability only and
 * is intentionally NOT used as a filter predicate (a timestamp suffix would
 * risk deleting the freshly inserted replacement message).
 */
export const captureBarrier = (
  list: TMessage[],
  targetMessageId: MessageId,
  targetCreatedAt: number
): EditResubmitBarrierCapture | null => {
  const targetIndex = list.findIndex(
    (message) =>
      message.position === 'right' &&
      message.type === 'text' &&
      (message.message_id === targetMessageId || message.msg_id === targetMessageId)
  );
  if (targetIndex < 0) return null;

  const suffix = list.slice(targetIndex);
  const mergeKeys = new Set<string>();
  const serverIds = new Set<string>();
  const localIds = new Set<string>();
  for (const message of suffix) {
    const key = getFetchedMergeKey(message);
    if (key) mergeKeys.add(key);
    if (message.message_id) serverIds.add(message.message_id);
    if (!message.msg_id) localIds.add(message.id);
  }
  return { mergeKeys, serverIds, localIds, cutoffCreatedAt: targetCreatedAt };
};

/** Insert an `armed` barrier for this operation. The backend request owns the
 * future: while armed, stale fetched rows are filtered but the current list is
 * left untouched (the old suffix is still legitimate until success). */
export const armBarrier = (
  conversationId: ConversationId,
  operationId: string,
  capture: EditResubmitBarrierCapture
): void => {
  const coordinator = getOrCreate(conversationId);
  coordinator.barriers.set(operationId, {
    operationId,
    mergeKeys: capture.mergeKeys,
    serverIds: capture.serverIds,
    localIds: capture.localIds,
    cutoffCreatedAt: capture.cutoffCreatedAt,
    phase: 'armed',
    pendingConsumers: new Set(),
  });
  // C4: 屏障建立（请求前）。Barrier armed (before the request).
  console.debug('[conversation-message-coordinator]', `arm conv=${conversationId}`, {
    operationId,
    mergeKeys: capture.mergeKeys.size,
    serverIds: capture.serverIds.size,
    localIds: capture.localIds.size,
    cutoffCreatedAt: capture.cutoffCreatedAt,
  });
};

/**
 * Atomically: epoch+1 → stamp `successEpoch` → flip to `reconciling` → snapshot
 * every active consumer into `pendingConsumers` → return the new epoch. A
 * single authoritative fetch at the latest epoch simultaneously confirms every
 * earlier reconciling barrier (two successive edits need only one refresh).
 */
export const beginEditResubmitReconciliation = (
  conversationId: ConversationId,
  operationId: string
): number => {
  const coordinator = getOrCreate(conversationId);
  coordinator.epoch += 1;
  const newEpoch = coordinator.epoch;
  const barrier = coordinator.barriers.get(operationId);
  if (barrier) {
    barrier.phase = 'reconciling';
    barrier.successEpoch = newEpoch;
    barrier.pendingConsumers = new Set(coordinator.consumers);
  }
  // A transition: if every consumer has already gone (e.g. the capturing
  // instance unmounted before the success callback landed), the empty pending
  // set means nothing will ever ack — retire the barrier now rather than leak.
  maybeDestroy(conversationId);
  // C4: 屏障翻转（成功后原子完成）。Barrier flipped to reconciling (post-acceptance).
  console.debug('[conversation-message-coordinator]', `reconcile conv=${conversationId}`, {
    operationId,
    epoch: newEpoch,
    pendingConsumers: coordinator.consumers.size,
  });
  return newEpoch;
};

/** Stop filtering for this operation (backend failed). The caller is
 * responsible for emitting the failed-refresh event; this only drops the
 * barrier. */
export const revokeBarrier = (conversationId: ConversationId, operationId: string): void => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator) return;
  coordinator.barriers.delete(operationId);
  // C4: 屏障撤销（请求失败）。Barrier revoked (request failed).
  console.debug('[conversation-message-coordinator]', `revoke conv=${conversationId}`, { operationId });
  maybeDestroy(conversationId);
};

/**
 * Ack that `consumerId` has applied a replace fetch whose epoch is
 * `appliedEpoch`. Every reconciling barrier whose `successEpoch` is
 * `<= appliedEpoch` drops this consumer; barriers whose pending set becomes
 * empty are deleted. Ends with `maybeDestroy`.
 */
export const ackConsumerReconciled = (
  conversationId: ConversationId,
  consumerId: ConsumerId,
  appliedEpoch: number
): void => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator) return;
  const retired: string[] = [];
  for (const barrier of coordinator.barriers.values()) {
    if (
      barrier.phase === 'reconciling' &&
      barrier.successEpoch !== undefined &&
      appliedEpoch >= barrier.successEpoch
    ) {
      barrier.pendingConsumers.delete(consumerId);
      if (barrier.pendingConsumers.size === 0) {
        coordinator.barriers.delete(barrier.operationId);
        retired.push(barrier.operationId);
      }
    }
  }
  // C4: 消费者收敛确认（一次最新权威 fetch 可同时确认多个屏障）。
  // Consumer acked a post-success authoritative fetch (one fetch may retire
  // several earlier barriers).
  if (retired.length > 0) {
    console.debug('[conversation-message-coordinator]', `ack conv=${conversationId}`, {
      consumerId,
      appliedEpoch,
      retired,
    });
  }
  maybeDestroy(conversationId);
};

/**
 * Drop fetched DB rows that belong to any active (armed or reconciling)
 * barrier. Fetched rows are durable, so only mergeKeys/serverIds apply —
 * localIds live in a different id space and never match a DB row.
 */
export const filterFetchedRows = (
  rows: TMessage[],
  conversationId: ConversationId
): TMessage[] => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator || !coordinator.barriers.size) return rows;

  const mergeKeys = new Set<string>();
  const serverIds = new Set<string>();
  for (const barrier of coordinator.barriers.values()) {
    for (const key of barrier.mergeKeys) mergeKeys.add(key);
    for (const id of barrier.serverIds) serverIds.add(id);
  }
  if (!mergeKeys.size && !serverIds.size) return rows;

  const operationIds = Array.from(coordinator.barriers.keys());
  const filtered = rows.filter((row) => {
    const key = getFetchedMergeKey(row);
    if (key && mergeKeys.has(key)) return false;
    if (row.message_id && serverIds.has(row.message_id)) return false;
    return true;
  });
  // C4: 过滤命中（陈旧 fetch 的旧后缀行被丢弃）。Fetched rows dropped (stale
  // suffix from a pre-truncate fetch).
  if (filtered.length !== rows.length) {
    console.debug('[conversation-message-coordinator]', `filtered conv=${conversationId}`, {
      dropped: rows.length - filtered.length,
      of: rows.length,
      barriers: operationIds,
    });
  }
  return filtered;
};

/**
 * Drop rows from a consumer's *current* in-memory list that belong to any
 * `reconciling` barrier. Unlike `filterFetchedRows`, this also removes
 * stream-only rows via localIds (which only the capturing instance can match).
 * Armed barriers deliberately leave the current list alone.
 */
export const purgeCurrentRows = (
  list: TMessage[],
  conversationId: ConversationId
): TMessage[] => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator || !coordinator.barriers.size) return list;

  const mergeKeys = new Set<string>();
  const serverIds = new Set<string>();
  const localIds = new Set<string>();
  let active = false;
  for (const barrier of coordinator.barriers.values()) {
    if (barrier.phase !== 'reconciling') continue;
    active = true;
    for (const key of barrier.mergeKeys) mergeKeys.add(key);
    for (const id of barrier.serverIds) serverIds.add(id);
    for (const id of barrier.localIds) localIds.add(id);
  }
  if (!active || (!mergeKeys.size && !serverIds.size && !localIds.size)) return list;

  const reconcilingOperations = Array.from(coordinator.barriers.values())
    .filter((barrier) => barrier.phase === 'reconciling')
    .map((barrier) => barrier.operationId);
  const purged = list.filter((row) => {
    const key = getFetchedMergeKey(row);
    if (key && mergeKeys.has(key)) return false;
    if (row.message_id && serverIds.has(row.message_id)) return false;
    if (localIds.has(row.id)) return false;
    return true;
  });
  // C4: 当前列表清理命中（消费者收敛前移除旧后缀）。Current list purged
  // (old suffix removed before the consumer converges).
  if (purged.length !== list.length) {
    console.debug('[conversation-message-coordinator]', `purged conv=${conversationId}`, {
      removed: list.length - purged.length,
      of: list.length,
      barriers: reconcilingOperations,
    });
  }
  return purged;
};

/**
 * Register a consumer (a mounted message-list instance). The returned `release`
 * must be called on unmount. A consumer that mounts while a barrier is already
 * reconciling joins that barrier's pending set, so it must complete its own
 * post-success authoritative load before the barrier can retire.
 */
export const retainConsumer = (
  conversationId: ConversationId
): { consumerId: ConsumerId; release: () => void } => {
  const coordinator = getOrCreate(conversationId);
  const consumerId = `c${nextConsumerSeq++}`;
  coordinator.consumers.add(consumerId);
  for (const barrier of coordinator.barriers.values()) {
    if (barrier.phase === 'reconciling') barrier.pendingConsumers.add(consumerId);
  }

  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    const current = coordinators.get(conversationId);
    if (!current) return;
    current.consumers.delete(consumerId);
    for (const barrier of current.barriers.values()) {
      barrier.pendingConsumers.delete(consumerId);
    }
    maybeDestroy(conversationId);
  };
  return { consumerId, release };
};

/**
 * Lifecycle reclamation, run after every state transition (release / ack /
 * revoke / in-flight callback landing):
 *  - an `armed` barrier keeps the coordinator alive (the in-flight request
 *    still owns the future; Preflight item 7 records the settle assumption);
 *  - once no consumers remain, the coordinator (and any reconciling barriers)
 *    is destroyed — remount reads the already-truncated DB directly.
 */
export const maybeDestroy = (conversationId: ConversationId): void => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator) return;

  // Pending sets are a subset of consumers; drop any reconciling barrier whose
  // last pending consumer has already been released/acked.
  for (const barrier of coordinator.barriers.values()) {
    if (barrier.phase !== 'reconciling') continue;
    for (const consumerId of barrier.pendingConsumers) {
      if (!coordinator.consumers.has(consumerId)) barrier.pendingConsumers.delete(consumerId);
    }
    if (barrier.pendingConsumers.size === 0) coordinator.barriers.delete(barrier.operationId);
  }

  const hasArmed = Array.from(coordinator.barriers.values()).some(
    (barrier) => barrier.phase === 'armed'
  );
  if (hasArmed) return;
  if (coordinator.consumers.size === 0) {
    coordinators.delete(conversationId);
    // C4: 协调器销毁（无消费者且无在飞请求）。Coordinator destroyed (no consumers,
    // no in-flight request).
    console.debug('[conversation-message-coordinator]', `destroy conv=${conversationId}`);
  }
};

/** Read-only snapshot for tests/observability. */
export const describeConversation = (
  conversationId: ConversationId
): {
  epoch: number;
  consumerCount: number;
  barriers: Array<{
    operationId: string;
    phase: 'armed' | 'reconciling';
    successEpoch?: number;
    pendingConsumers: ConsumerId[];
  }>;
} => {
  const coordinator = coordinators.get(conversationId);
  if (!coordinator) return { epoch: 0, consumerCount: 0, barriers: [] };
  return {
    epoch: coordinator.epoch,
    consumerCount: coordinator.consumers.size,
    barriers: Array.from(coordinator.barriers.values()).map((barrier) => ({
      operationId: barrier.operationId,
      phase: barrier.phase,
      successEpoch: barrier.successEpoch,
      pendingConsumers: Array.from(barrier.pendingConsumers),
    })),
  };
};

/** Test-only: clear all coordinators. */
export const __resetConversationMessageCoordinators = (): void => {
  coordinators.clear();
  nextConsumerSeq = 1;
};
