/**
 * Decoders for conversation activity payloads.
 *
 * The App Server delivers activity bodies as loosely-typed JSON: a field may
 * arrive at the top level, nested under `content`, or serialized as a JSON
 * string. These decoders are the single place that absorbs that variance so
 * the rest of the UI sees shaped data.
 *
 * `NOISE_ACTIVITY_KINDS` is the one authority on which activity kinds are
 * internal lifecycle noise rather than chat content. Both the event reducer
 * (`src/lib/conversation-events.ts`) and the renderer
 * (`src/components/messages/ActivityItem.tsx`) filter on it — a second copy
 * of that list drifts and leaks phantom rows into the transcript.
 */

export type Activity = {
  id: string;
  kind: string;
  createdAt: number;
  content?: unknown;
  status?: string | null;
  subject?: string | null;
  duration?: number | null;
};

export type ThinkingData = { content: string; subject: string | null; status: string | null; duration: number | null };
export type TipsData = { content: string; tipType: string };
export type ToolCallData = { name: string | null; args?: unknown; output?: unknown; status: string | null };

/**
 * Internal lifecycle noise: agent start/finish/status/error heartbeats and
 * turn boundary markers are not chat content. Terminal errors surface through
 * the `message.error` card instead.
 */
export const NOISE_ACTIVITY_KINDS: ReadonlySet<string> = new Set([
  "start",
  "finish",
  "error",
  "turn_started",
  "turn_completed",
]);

export function isNoiseActivityKind(kind: string): boolean {
  return NOISE_ACTIVITY_KINDS.has(kind);
}

export function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

export function numberValue(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

export function contentToText(value: unknown): string {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "content" in value && typeof (value as { content?: unknown }).content === "string") return (value as { content: string }).content;
  return value == null ? "" : JSON.stringify(value);
}

export function isActivityMessageType(messageType: string): boolean {
  return messageType === "thinking" || messageType === "tool_call" || messageType === "tool_group" || messageType === "plan" || messageType === "tips" || messageType === "acp_tool_call" || messageType === "agent_status";
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
  if (value && typeof value === "object" && "content" in value) {
    const content = (value as { content?: unknown }).content;
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

export function thinkingData(value: unknown): ThinkingData {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const direct = value as Record<string, unknown>;
    if (typeof direct.content === "string") {
      return {
        content: direct.content,
        subject: stringValue(direct.subject),
        status: stringValue(direct.status),
        duration: numberValue(direct.duration) ?? numberValue(direct.duration_ms),
      };
    }
  }
  const candidate = unwrapContent(value);
  if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) {
    return { content: typeof candidate === "string" ? candidate : "", subject: null, status: null, duration: null };
  }
  const data = candidate as Record<string, unknown>;
  return {
    content: stringValue(data.content) ?? "",
    subject: stringValue(data.subject),
    status: stringValue(data.status),
    duration: numberValue(data.duration) ?? numberValue(data.duration_ms),
  };
}

export function tipsData(value: unknown): TipsData {
  const readTip = (data: Record<string, unknown>): TipsData | null => {
    const content = stringValue(data.content);
    const tipType = stringValue(data.tip_type) ?? stringValue(data.type);
    if (content === null && tipType === null) return null;
    return { content: content ?? "", tipType: tipType ?? "info" };
  };
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const direct = readTip(value as Record<string, unknown>);
    if (direct) return direct;
  }
  const candidate = unwrapContent(value);
  if (candidate && typeof candidate === "object" && !Array.isArray(candidate)) {
    const parsed = readTip(candidate as Record<string, unknown>);
    if (parsed) return parsed;
  }
  if (candidate && typeof candidate !== "object") return { content: String(candidate), tipType: "info" };
  return { content: "", tipType: "info" };
}

export function toolCallData(value: unknown): ToolCallData | null {
  const candidate = unwrapContent(value);
  if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) return null;
  const data = candidate as Record<string, unknown>;
  const name = stringValue(data.name) ?? stringValue(data.tool_name) ?? stringValue(data.tool);
  if (!name && data.args === undefined && data.arguments === undefined && data.output === undefined && data.result === undefined) return null;
  return {
    name,
    args: data.args ?? data.arguments ?? data.input,
    output: data.output ?? data.result,
    status: stringValue(data.status),
  };
}
