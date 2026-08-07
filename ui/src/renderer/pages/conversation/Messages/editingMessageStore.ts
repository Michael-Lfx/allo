import { useSyncExternalStore } from 'react';

import type { ConversationId, MessageId } from '@/common/types/ids';

export type EditingMessagePhase = 'editing' | 'submitting' | 'confirming';

/**
 * 跨树共享的「正在编辑/重发」气泡状态（SendBox 写入，MessageText 读取）。
 * SendBox 与 MessageText 不构成父子关系，故用模块级 store + useSyncExternalStore
 * 而非 Provider/context。
 *
 * Cross-tree shared "editing / resubmitting" bubble state (SendBox writes,
 * MessageText reads). SendBox and MessageText are not parent/child, so this uses
 * a module-level store + useSyncExternalStore rather than a Provider/context.
 */
export interface EditingMessageState {
  /** 写入该状态的 SendBox 实例 id；仅 owner 匹配时可清除/更新（双实例护栏）。 */
  /** The SendBox instance id that wrote this state; only the owner may clear or
   * update it (dual-instance guard). */
  ownerId: string;
  /** 正在编辑/重发的消息 durable id（message_id ?? msg_id）。 */
  /** The durable id (message_id ?? msg_id) of the message being edited/resent. */
  msgId: MessageId;
  /** True 表示编辑重发请求正在飞行中；False 表示仅回填待提交。 */
  /** True while an edit-resubmit request is in flight; False while merely
   * recalled into the composer, not yet submitted. */
  pending: boolean;
  /** Explicit lifecycle phase; optional for compatibility with older store snapshots. */
  phase?: EditingMessagePhase;
  /** Wake the current same-key confirmation attempt; never starts a new edit. */
  continueConfirmation?: () => void;
  /** 当前飞行操作的 operation token（与 SendBox 的 activeEditOperationRef 对齐）。 */
  /** The in-flight operation token (aligned with SendBox's activeEditOperationRef). */
  operationId?: string;
}

type EditingMessageStore = Map<ConversationId, EditingMessageState>;

let store: EditingMessageStore = new Map();
const listeners = new Set<() => void>();

const emit = (): void => {
  for (const listener of listeners) listener();
};

// useSyncExternalStore 用 Object.is 比较 getSnapshot 返回值；每次写入替换整个 Map
// 引用，故无变更时引用稳定（不触发重渲染循环），有变更时引用变化触发订阅者更新。
// useSyncExternalStore compares getSnapshot's return with Object.is; we replace the
// whole Map reference on every write, so the reference is stable when nothing changed
// (no re-render loop) and changes when something did (subscribers update).
const replaceStore = (next: EditingMessageStore): void => {
  store = next;
  emit();
};

const subscribe = (listener: () => void): (() => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

const getSnapshot = (): EditingMessageStore => store;

/**
 * useSyncExternalStore 原语（导出以便测试驱动订阅语义）。
 * useSyncExternalStore primitives (exported so tests can drive subscription
 * semantics without a React render).
 */
export const subscribeEditingMessage = subscribe;
export const getEditingMessageSnapshot = getSnapshot;

/**
 * 进入编辑态：写入（或覆盖）该会话的编辑状态。
 * Enter edit mode: write (or overwrite) the editing state for this conversation.
 */
export const setEditingMessage = (
  conversationId: ConversationId,
  state: EditingMessageState
): void => {
  const next = new Map(store);
  next.set(conversationId, state);
  replaceStore(next);
};

/**
 * 仅 owner 匹配时更新；否则 no-op（防止已卸载/陈旧实例覆盖当前编辑者）。
 * Update only when the owner matches; otherwise no-op (prevents an unmounted or
 * stale instance from clobbering the current editor).
 */
export const updateEditingMessage = (
  conversationId: ConversationId,
  ownerId: string,
  patch: Partial<Omit<EditingMessageState, 'ownerId'>>
): void => {
  const existing = store.get(conversationId);
  if (!existing || existing.ownerId !== ownerId) return;
  const next = new Map(store);
  next.set(conversationId, { ...existing, ...patch });
  replaceStore(next);
};

/**
 * 仅 owner 匹配时清除；否则 no-op。
 * Clear only when the owner matches; otherwise no-op.
 */
export const clearEditingMessage = (conversationId: ConversationId, ownerId: string): void => {
  const existing = store.get(conversationId);
  if (!existing || existing.ownerId !== ownerId) return;
  const next = new Map(store);
  next.delete(conversationId);
  replaceStore(next);
};

/** 非响应式读取（用于一次性判定 / 测试）。Non-reactive read (one-off checks / tests). */
export const getEditingMessage = (conversationId: ConversationId): EditingMessageState | undefined =>
  store.get(conversationId);

/**
 * 响应式订阅某会话的编辑状态；无则 undefined。
 * Reactive subscription to a conversation's editing state (undefined if none).
 */
export const useEditingMessage = (
  conversationId: ConversationId | undefined
): EditingMessageState | undefined => {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot);
  if (!conversationId) return undefined;
  return snapshot.get(conversationId);
};

/** 仅测试用重置。Test-only reset. */
export const __resetEditingMessageStore = (): void => {
  replaceStore(new Map());
};
