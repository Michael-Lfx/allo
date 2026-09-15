import { CheckCircle2, Loader2, Terminal, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ThinkingItem } from "./ThinkingItem";
import { TipsItem } from "./TipsItem";
import { ToolCallItem } from "./ToolCallItem";
import {
  isContextCompressionTip,
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

/** 计划工具：它的参数就是计划本身，行由 `PlanPanel` 代表（与落库时隐藏同一条规则）。 */
const PLAN_TOOL_NAME = "update_plan";

export function ActivityItem({ activity }: { activity: Activity }) {
  const { t } = useTranslation();
  if (isNoiseActivityKind(activity.kind)) return null;
  // 计划不占会话流里的一行：它由输入框上方的 `PlanPanel` 常驻显示（同一份 `entries`
  // 换个位置，不再随历史滚走）。**一律**不渲染——兜底成「Agent 活动：plan」对用户
  // 没有任何信息量，正是这次要消掉的那一行。
  if (activity.kind === "plan") return null;
  const thinking = thinkingData(activity.content);
  if (activity.kind === "thinking") return <ThinkingItem activity={activity} thinking={thinking} />;
  if (activity.kind === "tips") {
    const tip = tipsData(activity.content);
    // 上下文压缩的 info 行不占一行：`/compact` 已有专门的压缩提示条表达。
    if (isContextCompressionTip(tip.tipType, tip.content)) return null;
    return <TipsItem tip={tip} />;
  }
  const tool = toolCallData(activity.content);
  // 计划工具的原始调用不单独成行：落库路径本来就把它标成 hidden（`persist_plan_source_tool`），
  // 只有实时路径会漏出来——于是同一轮对话在流式时多一行 `update_plan {…}`、刷新后又消失。
  if (tool?.name === PLAN_TOOL_NAME) return null;
  if (tool) return <ToolCallItem activity={activity} tool={tool} />;
  return <div className="activity-row">
    <span className={`activity-glyph is-${activity.status ?? "complete"}`} aria-hidden="true">
      {activity.status === "failed" || activity.status === "error" ? <XCircle size={14} strokeWidth={1.9} /> : activity.status === "running" || activity.status === "pending" ? <Loader2 size={14} className="spin" /> : <CheckCircle2 size={14} strokeWidth={1.9} />}
    </span>
    <span>{activityLabel(t, activity.kind)}</span>
    {activity.status && <span className="activity-status">{activity.status}</span>}
  </div>;
}
