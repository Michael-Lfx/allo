/**
 * Activity helpers for the transcript.
 *
 * The payload decoders themselves live in the package
 * (`@flowy-agent-store/protocol`, doc `16` R1) so every client shares one
 * definition of how a loosely-typed body is shaped. What stays here is
 * presentation policy: which activity kinds are internal lifecycle noise
 * rather than chat content.
 */

export {
  contentToText,
  isRecord,
  numberValue,
  parseContextUsage,
  stringValue,
  thinkingData,
  tipsData,
  toolCallData,
} from "./protocol";
export type { ThinkingData, TipsData, ToolCallData } from "./protocol";

export type Activity = {
  id: string;
  kind: string;
  createdAt: number;
  content?: unknown;
  status?: string | null;
  subject?: string | null;
  duration?: number | null;
};

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

export function isActivityMessageType(messageType: string): boolean {
  return messageType === "thinking" || messageType === "tool_call" || messageType === "tool_group" || messageType === "plan" || messageType === "tips" || messageType === "acp_tool_call" || messageType === "agent_status";
}
