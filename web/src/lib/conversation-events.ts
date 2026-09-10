/**
 * Realtime conversation stream: one pure reducer behind one seam.
 *
 * Everything that can change the visible transcript arrives here — server
 * events from the subscription, the optimistic row a send appends, and the
 * receipt that later reconciles it. Keeping them in one reducer is what makes
 * the ordering rules testable and keeps the optimistic-send reconciliation
 * next to the event merge it has to agree with.
 *
 * The reducer is pure: it never touches React. The component that owns the
 * subscription drives it with `useReducer`, which is what preserves the
 * at-most-once per event behaviour that functional state updaters gave us.
 */

import {
  contentToText,
  isNoiseActivityKind,
  numberValue,
  stringValue,
  thinkingData,
} from "./activity";
import type {
  ContextUsage,
  ConversationEvent,
  ConversationMessage,
  ConversationView,
} from "./protocol";

export type ConversationStreamState = {
  messages: ConversationMessage[];
  isProcessing: boolean;
  /**
   * Latest `context.usage` projection. A new object on every event so the
   * shell can mirror it into the conversation list with an effect.
   */
  contextUsage: { conversationId: string; usage: ContextUsage | null } | null;
  /** Keyset of the oldest loaded message; null before first load / when unknown. */
  historyCursor: string | null;
  /** Whether an earlier page may exist (true when the last page came back full-sized). */
  hasMore: boolean;
  /** Guard against concurrent older-page loads. */
  loadingOlder: boolean;
};

export type ConversationStreamAction =
  /** Replace the transcript: switching chats, loading history, disconnecting. */
  | { type: "reset"; messages: ConversationMessage[]; isProcessing?: boolean }
  /** Prepend an earlier page of history (loaded via cursor). Merged by id, ascending. */
  | { type: "prependHistory"; messages: ConversationMessage[] }
  /** One server event from the conversation subscription. */
  | { type: "event"; event: ConversationEvent }
  /** Optimistic user row, before the send receipt lands. */
  | { type: "appendPending"; message: ConversationMessage }
  /**
   * Send receipt arrived. The server can publish `message.created` before the
   * receipt reaches this browser, so if the real id is already present we drop
   * the optimistic row rather than rename it into a duplicate.
   */
  | { type: "reconcilePending"; pendingId: string; messageId: string; completed: boolean }
  /** Send failed: keep the row, mark it. */
  | { type: "failPending"; pendingId: string }
  /** Turn state changed on its own (cancel, or an authoritative read). */
  | { type: "setProcessing"; isProcessing: boolean };

export const initialConversationStream: ConversationStreamState = {
  messages: [],
  isProcessing: false,
  contextUsage: null,
  historyCursor: null,
  hasMore: false,
  loadingOlder: false,
};

export function conversationStreamReducer(
  state: ConversationStreamState,
  action: ConversationStreamAction,
): ConversationStreamState {
  switch (action.type) {
    case "reset":
      return {
        ...state,
        messages: action.messages,
        isProcessing: action.isProcessing ?? state.isProcessing,
        historyCursor: null,
        hasMore: false,
        loadingOlder: false,
      };
    case "prependHistory":
      // Older page: merge by id (dedup) and let the ascending sort slot it in
      // before the existing messages. Same merge path the realtime events use,
      // so history and live deltas can never disagree on ordering or duplicates.
      return { ...state, messages: mergeMessagesById(state.messages, action.messages) };
    case "event":
      return applyEvent(state, action.event);
    case "appendPending":
      return { ...state, messages: [...state.messages, action.message] };
    case "reconcilePending":
      return {
        ...state,
        messages: state.messages.some((message) => message.message_id === action.messageId)
          ? state.messages.filter((message) => message.message_id !== action.pendingId)
          : state.messages.map((message) => message.message_id === action.pendingId
            ? { ...message, message_id: action.messageId, status: "sent" }
            : message),
        isProcessing: !action.completed,
      };
    case "failPending":
      return {
        ...state,
        messages: state.messages.map((message) => message.message_id === action.pendingId
          ? { ...message, status: "failed" }
          : message),
      };
    case "setProcessing":
      return { ...state, isProcessing: action.isProcessing };
  }
}

function applyEvent(state: ConversationStreamState, event: ConversationEvent): ConversationStreamState {
  if (event.event_type === "context.usage") {
    return {
      ...state,
      contextUsage: {
        conversationId: event.conversation_id,
        usage: parseContextUsage(event.payload.context_usage),
      },
    };
  }
  if (event.event_type === "message.created") {
    const id = stringValue(event.payload.message_id);
    if (!id) return state;
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "user",
        content: event.payload.content ?? "",
        message_type: "text",
        created_at: numberValue(event.payload.created_at) ?? Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.delta") {
    const id = stringValue(event.payload.message_id);
    const delta = stringValue(event.payload.content) ?? "";
    const replace = event.payload.replace === true;
    if (!id) return state;
    const existing = state.messages.find((message) => message.message_id === id);
    const prior = existing ? contentToText(existing.content) : "";
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "assistant",
        content: replace ? delta : `${prior}${delta}`,
        message_type: "text",
        created_at: existing?.created_at ?? Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.tips") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:tips`;
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "activity",
        content: { content: stringValue(event.payload.content) ?? "", tip_type: stringValue(event.payload.tip_type) ?? "info" },
        message_type: "tips",
        created_at: numberValue(event.payload.created_at) ?? Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.tool") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:tool`;
    const existing = state.messages.find((message) => message.message_id === id);
    // A tool call streams as several events (running → completed). Later frames
    // may omit what an earlier one carried, so merge field-by-field with what is
    // already on screen instead of replacing the row.
    const prior = existing && existing.content && typeof existing.content === "object"
      ? existing.content as Record<string, unknown>
      : null;
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "activity",
        content: {
          name: stringValue(event.payload.name) ?? stringValue(prior?.name),
          args: event.payload.args ?? prior?.args,
          output: event.payload.output ?? prior?.output,
          status: stringValue(event.payload.status) ?? stringValue(prior?.status),
        },
        message_type: "tool_call",
        created_at: existing?.created_at ?? Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.error") {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:error`;
    const errorMessage = stringValue(event.payload.message) ?? "模型返回了错误";
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "assistant",
        content: errorMessage,
        message_type: "error",
        status: "error",
        created_at: Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.thinking" || (event.event_type === "message.activity" && event.payload.kind === "thinking")) {
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:thinking`;
    const content = stringValue(event.payload.content) ?? "";
    const replace = event.payload.replace === true;
    const existing = state.messages.find((message) => message.message_id === id);
    const prior = existing ? thinkingData(existing.content).content : "";
    const nextContent = replace ? content : `${prior}${content}`;
    const priorThinking = existing ? thinkingData(existing.content) : null;
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "activity",
        content: {
          content: nextContent,
          subject: stringValue(event.payload.subject) ?? priorThinking?.subject,
          status: stringValue(event.payload.status) ?? priorThinking?.status,
          duration: numberValue(event.payload.duration) ?? priorThinking?.duration,
        },
        message_type: "thinking",
        created_at: existing?.created_at ?? Date.now(),
      }]),
    };
  }
  if (event.event_type === "message.activity") {
    const kind = stringValue(event.payload.kind) ?? "activity";
    if (isNoiseActivityKind(kind)) return state;
    const id = stringValue(event.payload.message_id) ?? `${event.conversation_id}:${event.sequence}`;
    return {
      ...state,
      messages: mergeMessagesById(state.messages, [{
        message_id: id,
        conversation_id: event.conversation_id,
        role: "activity",
        content: event.payload.content ?? "",
        message_type: kind,
        created_at: Date.now(),
      }]),
    };
  }
  if (event.event_type === "turn.status") {
    return { ...state, isProcessing: stringValue(event.payload.status) === "running" };
  }
  return state;
}

/** Merge by `message_id`, newest last. Later fields win on collision. */
export function mergeMessagesById(
  current: ConversationMessage[],
  incoming: ConversationMessage[],
): ConversationMessage[] {
  const next = new Map(current.map((message) => [message.message_id, message]));
  for (const message of incoming) {
    next.set(message.message_id, { ...next.get(message.message_id), ...message });
  }
  return [...next.values()].sort((left, right) => left.created_at - right.created_at);
}

export function upsertConversation(
  items: ConversationView[],
  value: ConversationView,
): ConversationView[] {
  return [value, ...items.filter((item) => item.conversation_id !== value.conversation_id)].sort((left, right) => right.modified_at - left.modified_at);
}

/** Shape a `context.usage` payload; null when nothing was actually measured. */
export function parseContextUsage(value: unknown): ContextUsage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const data = value as Record<string, unknown>;
  const used = typeof data.used_tokens === "number" ? data.used_tokens : 0;
  const window = typeof data.window_tokens === "number" ? data.window_tokens : 0;
  if (used <= 0 || window <= 0) return null; // nothing measured → unknown
  const rawPercent = typeof data.percent === "number" ? data.percent : (used / window) * 100;
  return {
    used_tokens: used,
    window_tokens: window,
    percent: Math.min(100, Math.max(0, rawPercent)),
    updated_at: typeof data.updated_at === "number" ? data.updated_at : Date.now(),
    source: typeof data.source === "string" ? data.source : "measured",
  };
}
