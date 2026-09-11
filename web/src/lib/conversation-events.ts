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

import { decodeConversationEvent, isRecord } from "@flowy-agent-store/protocol";
import {
  contentToText,
  isNoiseActivityKind,
  stringValue,
  thinkingData,
  type ThinkingData,
} from "./activity";
import type {
  ContextUsage,
  ConversationEvent,
  ConversationMessage,
  ConversationView,
  TurnUsage,
} from "./protocol";

/** 本轮用量 + 它归属的模型键快照（数字来自 wire，模型键是接收时的上下文）。 */
export type TurnUsageSnapshot = {
  usage: TurnUsage;
  /** 接收时用户/会话所指的模型键（`provider/model`）；未知为 `null`（不算金额）。 */
  modelKey: string | null;
};

/** Reducer 保持纯函数：接收事件那一刻的会话上下文由调用方传入。 */
export type ConversationStreamContext = {
  /** 该事件归属的模型键（`provider/model`）；未知为 `null`。 */
  modelKey: string | null;
};

export type ConversationStreamState = {
  messages: ConversationMessage[];
  isProcessing: boolean;
  /**
   * R31（D-STREAM-2 ②）：本轮已收到 `turn_completed`，但回合还没真正结束（服务端
   * 此刻仍在收尾——`Finish` 要等记忆蒸馏 child，实测可达 10 秒）。忙态文案据此
   * 从「正在处理」升级为「正在收尾…」，**不改任何语义**：输入框该禁用还是禁用，
   * 只是不再让用户以为模型还在生成。
   */
  wrapUp: boolean;
  /**
   * W9（R14）：最近一次**本轮已完成**的 token 用量 + 模型键快照。
   *
   * 逐轮用量只存在于实时事件里（服务端不持久化历史轮次），所以这里刻意只保留
   * 本轮：新一轮开始即清空，没有上报就是 `null`——绝不拿上一轮的数字顶替本轮。
   */
  turnUsage: TurnUsageSnapshot | null;
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
  wrapUp: false,
  turnUsage: null,
  contextUsage: null,
  historyCursor: null,
  hasMore: false,
  loadingOlder: false,
};

export function conversationStreamReducer(
  state: ConversationStreamState,
  action: ConversationStreamAction,
  context: ConversationStreamContext = { modelKey: null },
): ConversationStreamState {
  switch (action.type) {
    case "reset":
      return {
        ...state,
        messages: action.messages,
        isProcessing: action.isProcessing ?? state.isProcessing,
        wrapUp: false,
        // 换会话 / 重载历史：逐轮用量不持久化，旧值属于另一个会话，必须丢。
        turnUsage: null,
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
      return applyEvent(state, action.event, context);
    case "appendPending":
      // 用户发出新一轮：上一轮的用量不再属于「本轮」。
      return { ...state, turnUsage: null, messages: [...state.messages, action.message] };
    case "reconcilePending":
      return {
        ...state,
        wrapUp: false,
        // 回执 `completed: false` = 新回合正在跑；`completed: true` 可能晚于
        // `turn_completed` 事件到达，此时必须保留刚记下的本轮用量。
        turnUsage: action.completed ? state.turnUsage : null,
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
      // R31：忙态结束时一并清掉「收尾中」标记，避免下一次忙态误用旧状态。
      return {
        ...state,
        isProcessing: action.isProcessing,
        wrapUp: action.isProcessing ? state.wrapUp : false,
        turnUsage: action.isProcessing ? null : state.turnUsage,
      };
  }
}

/**
 * Apply one frame to the transcript.
 *
 * The payload shapes come from the package decoder (`decodeConversationEvent`,
 * doc `16` R1) — what stays here is UI policy: the default row id an id-less
 * frame gets, and how a streaming frame merges with what is already on screen.
 */
function applyEvent(state: ConversationStreamState, event: ConversationEvent, context: ConversationStreamContext): ConversationStreamState {
  const decoded = decodeConversationEvent(event);
  switch (decoded.kind) {
    case "context.usage":
      return {
        ...state,
        contextUsage: { conversationId: event.conversation_id, usage: decoded.usage },
      };

    case "message.created": {
      if (!decoded.messageId) return state;
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: decoded.messageId,
          conversation_id: event.conversation_id,
          role: "user",
          content: decoded.content,
          message_type: "text",
          created_at: decoded.createdAt ?? Date.now(),
        }]),
      };
    }

    case "message.delta": {
      const id = decoded.messageId;
      if (!id) return state;
      const existing = state.messages.find((message) => message.message_id === id);
      const prior = existing ? contentToText(existing.content) : "";
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: id,
          conversation_id: event.conversation_id,
          role: "assistant",
          content: decoded.replace ? decoded.delta : `${prior}${decoded.delta}`,
          message_type: "text",
          created_at: existing?.created_at ?? Date.now(),
        }]),
      };
    }

    case "message.thinking":
      return applyThinking(
        state,
        event,
        decoded.thinking,
        decoded.replace,
        decoded.messageId ?? `${event.conversation_id}:thinking`,
        decoded.createdAt,
      );

    case "message.tool": {
      const id = decoded.messageId ?? `${event.conversation_id}:tool`;
      const existing = state.messages.find((message) => message.message_id === id);
      // A tool call streams as several events (running → completed). Later frames
      // may omit what an earlier one carried, so merge field-by-field with what is
      // already on screen instead of replacing the row.
      const prior = existing && isRecord(existing.content) ? existing.content : null;
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: id,
          conversation_id: event.conversation_id,
          role: "activity",
          content: {
            name: decoded.tool?.name ?? stringValue(prior?.name),
            args: decoded.tool?.args ?? prior?.args,
            output: decoded.tool?.output ?? prior?.output,
            status: decoded.tool?.status ?? stringValue(prior?.status),
          },
          message_type: "tool_call",
          created_at: existing?.created_at ?? Date.now(),
        }]),
      };
    }

    case "message.tips": {
      const id = decoded.messageId ?? `${event.conversation_id}:tips`;
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: id,
          conversation_id: event.conversation_id,
          role: "activity",
          content: { content: decoded.tips.content, tip_type: decoded.tips.tipType },
          message_type: "tips",
          created_at: decoded.createdAt ?? Date.now(),
        }]),
      };
    }

    case "message.error": {
      const id = decoded.messageId ?? `${event.conversation_id}:error`;
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: id,
          conversation_id: event.conversation_id,
          role: "assistant",
          // W7（R12）：`code` / `retryable` 随行带上——「重试」入口与「不可重试」标注
          // 都依赖它，先前只存了错误文本。
          content: {
            content: decoded.message ?? "模型返回了错误",
            code: decoded.code,
            retryable: decoded.retryable,
          },
          message_type: "error",
          status: "error",
          created_at: decoded.createdAt ?? Date.now(),
        }]),
      };
    }

    case "message.activity": {
      // Thinking frames were normalized to `message.thinking` by the decoder;
      // what is left here is the lifecycle heartbeats (filtered as noise) and
      // the real activity rows.
      if (isNoiseActivityKind(decoded.activityKind)) {
        // R31：`turn_completed` 不是渲染内容，但它是「模型答完了、服务端还在收尾」的
        // 唯一信号（D-STREAM-2：Finish 要等记忆蒸馏 child）。把它降级成一个
        // 布尔标记，忙态文案据此分级。
        if (decoded.activityKind !== "turn_completed") return state;
        // W9（R14）：它同时也是**本轮 token 用量**的唯一载体（运行时 `TurnCompleted`
        // → 服务端 `message.activity{kind:"turn_completed"}` 的 `usage`）。
        // 没有上报就是 `null`：宁可这一轮不显示，也不拿上一轮的数字顶替。
        return {
          ...state,
          wrapUp: true,
          turnUsage: decoded.usage ? { usage: decoded.usage, modelKey: context.modelKey } : null,
        };
      }
      const id = decoded.messageId ?? `${event.conversation_id}:${event.sequence}`;
      return {
        ...state,
        messages: mergeMessagesById(state.messages, [{
          message_id: id,
          conversation_id: event.conversation_id,
          role: "activity",
          content: decoded.content,
          message_type: decoded.activityKind,
          created_at: Date.now(),
        }]),
      };
    }

    case "turn.status":
      return { ...state, isProcessing: decoded.running, wrapUp: decoded.running ? state.wrapUp : false };

    case "unknown":
      // A kind this build does not know: nothing to render. Reaching this at
      // runtime is the only way past the closed `event_type` union.
      return state;
  }
}

/** Thinking-row merge shared by the dedicated frame and the normalized activity. */
function applyThinking(
  state: ConversationStreamState,
  event: ConversationEvent,
  thinking: ThinkingData,
  replace: boolean,
  id: string,
  serverCreatedAt: number | null,
): ConversationStreamState {
  const existing = state.messages.find((message) => message.message_id === id);
  const priorThinking = existing ? thinkingData(existing.content) : null;
  const nextContent = replace ? thinking.content : `${priorThinking?.content ?? ""}${thinking.content}`;
  return {
    ...state,
    messages: mergeMessagesById(state.messages, [{
      message_id: id,
      conversation_id: event.conversation_id,
      role: "activity",
      content: {
        content: nextContent,
        subject: thinking.subject ?? priorThinking?.subject,
        status: thinking.status ?? priorThinking?.status,
        duration: thinking.duration ?? priorThinking?.duration,
      },
      message_type: "thinking",
      created_at: existing?.created_at ?? serverCreatedAt ?? Date.now(),
    }]),
  };
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
