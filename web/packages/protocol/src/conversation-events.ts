/**
 * Conversation-event payload decoding (doc `16` R1).
 *
 * The wire carries `payload: Record<string, unknown>`: a field may arrive at
 * the top level, nested one level under `content`, or JSON-encoded as a string
 * (the server serializes ACP/provider bodies verbatim). These decoders are the
 * single place that absorbs that variance, so every client sees shaped bodies
 * instead of poking at unknown JSON in its own way.
 *
 * Decoding stops at the payload. Transcript identity (default row ids), merge
 * rules and user-facing copy stay in the consumer — they are UI policy, not
 * wire format.
 */

import type { ContextUsage, ConversationEvent, TurnUsage } from "./protocol";

export type ThinkingData = {
  content: string;
  subject: string | null;
  status: string | null;
  duration: number | null;
};

export type TipsData = { content: string; tipType: string };

export type ToolCallData = {
  name: string | null;
  args?: unknown;
  output?: unknown;
  status: string | null;
};

export function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

export function numberValue(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

/**
 * Strict boolean reader: a wire value that is not a real boolean reads as
 * `null` (unknown) rather than being coerced — "retryable" must never be
 * guessed from a truthy string.
 */
export function booleanValue(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

/** Flatten a message/activity body to plain text, JSON-encoding anything else. */
export function contentToText(value: unknown): string {
  if (typeof value === "string") return value;
  if (isRecord(value) && typeof value.content === "string") return value.content;
  return value == null ? "" : JSON.stringify(value);
}

/** Activity bodies may be nested one level down or JSON-encoded as a string. */
function unwrapContent(value: unknown): unknown {
  if (typeof value === "string") {
    try {
      return JSON.parse(value) as unknown;
    } catch {
      return value;
    }
  }
  if (isRecord(value)) {
    const content = value.content;
    if (typeof content === "string") {
      try {
        return JSON.parse(content) as unknown;
      } catch {
        return value;
      }
    }
  }
  return value;
}

/** Shape a thinking body (`message.thinking`, or `message.activity` kind `thinking`). */
export function thinkingData(value: unknown): ThinkingData {
  if (isRecord(value) && typeof value.content === "string") {
    return {
      content: value.content,
      subject: stringValue(value.subject),
      status: stringValue(value.status),
      duration: numberValue(value.duration) ?? numberValue(value.duration_ms),
    };
  }
  const candidate = unwrapContent(value);
  if (!isRecord(candidate)) {
    return { content: typeof candidate === "string" ? candidate : "", subject: null, status: null, duration: null };
  }
  return {
    content: stringValue(candidate.content) ?? "",
    subject: stringValue(candidate.subject),
    status: stringValue(candidate.status),
    duration: numberValue(candidate.duration) ?? numberValue(candidate.duration_ms),
  };
}

/** Shape a tips body; `type` is accepted as an alias of `tip_type`. */
export function tipsData(value: unknown): TipsData {
  const readTip = (data: Record<string, unknown>): TipsData | null => {
    const content = stringValue(data.content);
    const tipType = stringValue(data.tip_type) ?? stringValue(data.type);
    if (content === null && tipType === null) return null;
    return { content: content ?? "", tipType: tipType ?? "info" };
  };
  if (isRecord(value)) {
    const direct = readTip(value);
    if (direct) return direct;
  }
  const candidate = unwrapContent(value);
  if (isRecord(candidate)) {
    const parsed = readTip(candidate);
    if (parsed) return parsed;
  }
  if (candidate != null && !isRecord(candidate)) return { content: String(candidate), tipType: "info" };
  return { content: "", tipType: "info" };
}

/** Shape a tool-call body; `null` when the body carries nothing tool-shaped. */
export function toolCallData(value: unknown): ToolCallData | null {
  const candidate = unwrapContent(value);
  if (!isRecord(candidate)) return null;
  const name = stringValue(candidate.name) ?? stringValue(candidate.tool_name) ?? stringValue(candidate.tool);
  if (!name && candidate.args === undefined && candidate.arguments === undefined && candidate.output === undefined && candidate.result === undefined) {
    return null;
  }
  return {
    name,
    args: candidate.args ?? candidate.arguments ?? candidate.input,
    output: candidate.output ?? candidate.result,
    status: stringValue(candidate.status),
  };
}

/** Shape a `context.usage` payload; `null` when nothing was actually measured. */
export function parseContextUsage(value: unknown): ContextUsage | null {
  if (!isRecord(value)) return null;
  const used = typeof value.used_tokens === "number" ? value.used_tokens : 0;
  const window = typeof value.window_tokens === "number" ? value.window_tokens : 0;
  if (used <= 0 || window <= 0) return null; // nothing measured → unknown
  const rawPercent = typeof value.percent === "number" ? value.percent : (used / window) * 100;
  return {
    used_tokens: used,
    window_tokens: window,
    percent: Math.min(100, Math.max(0, rawPercent)),
    updated_at: typeof value.updated_at === "number" ? value.updated_at : Date.now(),
    source: typeof value.source === "string" ? value.source : "measured",
  };
}

/**
 * Shape a per-turn `usage` body (W9 / R14); `null` when the runtime reported
 * nothing usable.
 *
 * A missing side is **unknown**, not zero: half a bill is not a bill, so a
 * frame that only carries `input_tokens` decodes to `null` rather than to a
 * total that silently drops the output side. Same rule for an all-zero report —
 * "no tokens reported" must never render as "this turn was free".
 */
export function parseTurnUsage(value: unknown): TurnUsage | null {
  if (!isRecord(value)) return null;
  const input = typeof value.input_tokens === "number" ? value.input_tokens : null;
  const output = typeof value.output_tokens === "number" ? value.output_tokens : null;
  if (input === null || output === null) return null;
  if (input <= 0 && output <= 0) return null;
  const total = typeof value.total_tokens === "number" ? value.total_tokens : input + output;
  return { input_tokens: input, output_tokens: output, total_tokens: total };
}

/**
 * One decoded conversation event, discriminated by `kind`.
 *
 * `messageId` is the id the *server* supplied (`null` when it sent none) — a
 * consumer that needs a stable row for an id-less event applies its own
 * default, because that default is presentation, not protocol.
 *
 * `unknown` is the honest fall-through for a newer server kind: the raw
 * `eventType` is preserved so a caller can log or forward it instead of
 * silently dropping the frame.
 */
type MessageFrameBase = {
  event: ConversationEvent;
  /** The id the server supplied; `null` when it sent none (consumer picks a default row). */
  messageId: string | null;
  /** Server timestamp when the payload carried one. */
  createdAt: number | null;
};

export type DecodedConversationEvent =
  | ({ kind: "message.created"; content: unknown } & MessageFrameBase)
  | ({ kind: "message.delta"; delta: string; replace: boolean } & MessageFrameBase)
  | ({ kind: "message.thinking"; thinking: ThinkingData; replace: boolean } & MessageFrameBase)
  | ({ kind: "message.tool"; tool: ToolCallData | null } & MessageFrameBase)
  | ({ kind: "message.tips"; tips: TipsData } & MessageFrameBase)
  | ({ kind: "message.error"; message: string | null; code: string | null; retryable: boolean | null } & MessageFrameBase)
  | ({ kind: "message.activity"; activityKind: string; content: unknown; usage: TurnUsage | null } & MessageFrameBase)
  | { kind: "turn.status"; event: ConversationEvent; running: boolean }
  | { kind: "context.usage"; event: ConversationEvent; usage: ContextUsage | null }
  | { kind: "unknown"; event: ConversationEvent; eventType: string };

/**
 * Decode one `conversation/event` frame into a typed body.
 *
 * An activity frame whose `kind` is `thinking` is normalized to
 * `message.thinking` (the server has two spellings for the same thing).
 */
export function decodeConversationEvent(event: ConversationEvent): DecodedConversationEvent {
  const payload = event.payload ?? {};
  const messageId = () => stringValue(payload.message_id);
  const createdAt = () => numberValue(payload.created_at);
  // The server reports thinking under two spellings: a dedicated
  // `message.thinking` frame and an activity frame whose `kind` is `thinking`.
  // Normalize here, so every consumer keeps one thinking path.
  if (event.event_type === "message.activity" && stringValue(payload.kind) === "thinking") {
    return {
      kind: "message.thinking",
      event,
      messageId: messageId(),
      createdAt: createdAt(),
      thinking: thinkingData(payload),
      replace: payload.replace === true,
    };
  }
  switch (event.event_type) {
    case "message.created":
      return {
        kind: "message.created",
        event,
        messageId: messageId(),
        createdAt: createdAt(),
        content: payload.content ?? "",
      };
    case "message.delta":
      return {
        kind: "message.delta",
        event,
        messageId: messageId(),
        createdAt: createdAt(),
        delta: stringValue(payload.content) ?? "",
        replace: payload.replace === true,
      };
    case "message.thinking":
      return {
        kind: "message.thinking",
        event,
        messageId: messageId(),
        createdAt: createdAt(),
        thinking: thinkingData(payload),
        replace: payload.replace === true,
      };
    case "message.tool":
      return { kind: "message.tool", event, messageId: messageId(), createdAt: createdAt(), tool: toolCallData(payload) };
    case "message.tips":
      return { kind: "message.tips", event, messageId: messageId(), createdAt: createdAt(), tips: tipsData(payload) };
    case "message.error":
      return {
        kind: "message.error",
        event,
        messageId: messageId(),
        createdAt: createdAt(),
        message: stringValue(payload.message),
        // W7（R12）：app-server 的 `message.error` 投影本就带 `code` / `retryable`，
        // 解码器此前把它们丢掉了，于是「是否可重试」在界面上只能靠猜。
        code: stringValue(payload.code),
        retryable: booleanValue(payload.retryable),
      };
    case "message.activity": {
      const activityKind = stringValue(payload.kind) ?? "activity";
      return {
        kind: "message.activity",
        event,
        messageId: messageId(),
        createdAt: createdAt(),
        activityKind,
        content: payload.content ?? "",
        // W9（R14）：只有 `turn_completed` 活动带本轮用量（服务端投影如此，且只有它
        // 来自运行时的 `TurnCompleted`）。没有上报就是 `null`（未知）——不拿别的活动的
        // 字段或上一轮的数字顶替。其他活动即使夹带 `usage` 也不读。
        usage: activityKind === "turn_completed" ? parseTurnUsage(payload.usage) : null,
      };
    }
    case "turn.status":
      return { kind: "turn.status", event, running: stringValue(payload.status) === "running" };
    case "context.usage":
      return { kind: "context.usage", event, usage: parseContextUsage(payload.context_usage) };
    default:
      return { kind: "unknown", event, eventType: event.event_type as string };
  }
}
