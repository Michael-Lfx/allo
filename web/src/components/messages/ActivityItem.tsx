import { Terminal } from "lucide-react";
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

export function ActivityItem({ activity }: { activity: Activity }) {
  const { t } = useTranslation();
  if (isNoiseActivityKind(activity.kind)) return null;
  const thinking = thinkingData(activity.content);
  if (activity.kind === "thinking") return <ThinkingItem activity={activity} thinking={thinking} />;
  if (activity.kind === "tips") return <TipsItem tip={tipsData(activity.content)} />;
  const tool = toolCallData(activity.content);
  if (tool) return <ToolCallItem activity={activity} tool={tool} />;
  return <div className="activity-row"><span className="activity-glyph" aria-hidden="true"><Terminal size={14} strokeWidth={1.7} /></span><span>{activityLabel(t, activity.kind)}</span></div>;
}
