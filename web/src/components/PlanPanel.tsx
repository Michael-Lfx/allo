import { useId, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, ListChecks, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { latestPlan, planCounts, planPanelVisible } from "../lib/plan";
import { PlanSteps, PlanSummary } from "./PlanSteps";
import type { PlanData } from "../lib/activity";
import { useAppStore } from "../store/appStore";

/**
 * 输入框上方的任务面板（`update_plan` 的步骤清单）—— 它是**当下状态**的指示器：
 * 「这一轮还剩哪几步」。与它互补的是会话流里那条**历史行**（`messages/PlanItem`）：
 * 面板收起之后（回合结束 / 计划走完 / 用户关掉），那份计划仍然在流里查得到，而
 * 「同一份计划同时在两处各占一块」由 `lib/plan.ts` 的 `livePlanRowId` 挡掉。
 *
 * 什么时候出现由 `planPanelVisible` 决定。`key` 用计划行的 `message_id`：新计划回到
 * 默认展开态，也不继承上一条的关闭与折叠。
 */
export function PlanPanel() {
  const messages = useAppStore((s) => s.stream.messages);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const current = useMemo(() => latestPlan(messages), [messages]);
  const [dismissedId, setDismissedId] = useState<string | null>(null);
  if (!current || !planPanelVisible(current, isProcessing, dismissedId)) return null;
  const { plan, message } = current;
  return <PlanPanelCard
    key={message.message_id}
    plan={plan}
    onClose={() => setDismissedId(message.message_id)}
  />;
}

/** 面板本体：只认 `plan`（与可选的关闭回调），取数与可见性由 `PlanPanel` 负责。 */
export function PlanPanelCard({ plan, onClose }: { plan: PlanData; onClose?: () => void }) {
  const { t } = useTranslation();
  const stepsId = useId();
  const counts = planCounts(plan);

  // 「计划还没走完」直接由**步骤本身**判定，不看行状态：流式中的活动行没有 `status`
  // （只有 `persist_plan` 落库时才写），照行状态判会让进行中的计划默认收起。步骤是
  // 两条路径（实时 / 重新加载）都逐字一致的那份事实。
  const live = counts.completed < plan.entries.length;
  const [userExpanded, setUserExpanded] = useState<boolean | null>(null);
  const expanded = userExpanded ?? live;

  return <section className="plan-panel">
    {/* 关闭键必须是折叠键的**兄弟**：按钮不能嵌在按钮里。 */}
    <div className="plan-panel-head">
      <button
        type="button"
        className="plan-panel-toggle"
        aria-expanded={expanded}
        aria-controls={stepsId}
        onClick={() => setUserExpanded(!expanded)}
      >
        <ListChecks className="plan-panel-icon" size={15} strokeWidth={1.7} aria-hidden="true" />
        <span className="plan-panel-title">{t("activity.plan")}</span>
        <PlanSummary plan={plan} className="plan-panel-summary" />
        {expanded
          ? <ChevronDown className="plan-panel-caret" size={15} strokeWidth={1.7} aria-hidden="true" />
          : <ChevronRight className="plan-panel-caret" size={15} strokeWidth={1.7} aria-hidden="true" />}
      </button>
      {onClose && <button
        type="button"
        className="plan-panel-close"
        aria-label={t("activity.planClose")}
        title={t("activity.planClose")}
        onClick={onClose}
      >
        <X size={14} strokeWidth={1.8} aria-hidden="true" />
      </button>}
    </div>
    {expanded && <PlanSteps plan={plan} id={stepsId} />}
  </section>;
}
