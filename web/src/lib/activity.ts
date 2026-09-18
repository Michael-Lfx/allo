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
  planData,
  stringValue,
  thinkingData,
  tipsData,
  toolCallData,
} from "./protocol";
export type { PlanData, PlanEntry, PlanStepStatus, ThinkingData, TipsData, ToolCallData } from "./protocol";

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
 *
 * `agent_status` is the backend's per-turn **model activity** row
 * (`derived_message_id("agent_status", "model_activity")`，按 `msg_id` 一条)：
 * 每一轮都留一条「目标已完成」/「目标运行中」在 transcript 里，而它既不是内容、
 * 也不含用户需要的信息——按本集合的规则它本就该在这里，先前只是漏了。
 */
export const NOISE_ACTIVITY_KINDS: ReadonlySet<string> = new Set([
  "start",
  "finish",
  "error",
  "turn_started",
  "turn_completed",
  "agent_status",
]);

export function isNoiseActivityKind(kind: string): boolean {
  return NOISE_ACTIVITY_KINDS.has(kind);
}

export function isActivityMessageType(messageType: string): boolean {
  return messageType === "thinking" || messageType === "tool_call" || messageType === "tool_group" || messageType === "plan" || messageType === "tips" || messageType === "acp_tool_call" || messageType === "agent_status";
}

/**
 * 引擎的「上下文压缩」info 提示（`/compact`、autocompact、microcompact）不是聊天内容：
 * `/compact` 在 web 上已有专门的压缩提示条表达，而这类行在短会话里还会打印
 * `0 messages summarized` 的纯噪音。桌面端按同一模式把它归入 process trace
 * （`ui/.../processTipModel.ts` 的 `isContextCompressionTip`），web 直接不渲染。
 *
 * 只过滤 `info`：`Compact failed` / `Autocompact failed` 走 warning / error，是用户必须
 * 看到的信息，不能被这条规则吞掉。
 */
const CONTEXT_COMPRESSION_PATTERN = /\b(?:microcompact|autocompact|context compaction|context compact|compact(?:ed|ion)?)\b/i;

export function isContextCompressionTip(tipType: string, content: string): boolean {
  return tipType === "info" && CONTEXT_COMPRESSION_PATTERN.test(content);
}
