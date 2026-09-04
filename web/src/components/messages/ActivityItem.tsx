import { CheckCircle2, Loader2, Target, Terminal, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ThinkingItem } from "./ThinkingItem";
import { TipsItem } from "./TipsItem";
import { ToolCallItem } from "./ToolCallItem";
import {
  isNoiseActivityKind,
  thinkingData,
  tipsData,
  toolCallData,
  type Activity,
} from "../../lib/activity";

function activityLabel(t: (key: string, opts?: Record<string, unknown>) => string, kind: string): string {
  if (kind === "thinking") return t("activity.analyzing");
  if (kind === "tool_call") return t("activity.callingTool");
  if (kind === "tool_group") return t("activity.executingToolStep");
  return t("activity.agentActivity", { kind: kind.replace(/_/g, " ") });
}

/** "目标已暂停 / 目标运行中 / 目标已完成" — agent_status pill text. */
function agentStatusLabel(t: (key: string, opts?: Record<string, unknown>) => string, status: string | null | undefined, content: unknown): string {
  const text = typeof content === "string" && content.trim() ? content.trim() : "";
  if (text) return text;
  if (status === "running" || status === "active") return t("activity.goalRunning");
  if (status === "completed" || status === "done" || status === "finish") return t("activity.goalCompleted");
  return t("activity.goalPaused");
}

export function ActivityItem({ activity }: { activity: Activity }) {
  const { t } = useTranslation();
  if (isNoiseActivityKind(activity.kind)) return null;
  const thinking = thinkingData(activity.content);
  if (activity.kind === "thinking") return <ThinkingItem activity={activity} thinking={thinking} />;
  if (activity.kind === "tips") return <TipsItem tip={tipsData(activity.content)} />;
  if (activity.kind === "agent_status") return <div className={`agent-status-pill is-${activity.status ?? "paused"}`}>
    <Target size={14} strokeWidth={1.8} aria-hidden="true" />
    <span>{agentStatusLabel(t, activity.status, activity.content)}</span>
  </div>;
  const tool = toolCallData(activity.content);
  if (tool) return <ToolCallItem activity={activity} tool={tool} />;
  return <div className="activity-row">
    <span className={`activity-glyph is-${activity.status ?? "complete"}`} aria-hidden="true">
      {activity.status === "failed" || activity.status === "error" ? <XCircle size={14} strokeWidth={1.9} /> : activity.status === "running" || activity.status === "pending" ? <Loader2 size={14} className="spin" /> : <CheckCircle2 size={14} strokeWidth={1.9} />}
    </span>
    <span>{activityLabel(t, activity.kind)}</span>
    {activity.status && <span className="activity-status">{activity.status}</span>}
  </div>;
}
