import { useId, useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, Circle, ListChecks, Loader2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { latestPlan, planPanelVisible } from "../lib/plan";
import type { PlanData, PlanStepStatus } from "../lib/activity";
import { useAppStore } from "../store/appStore";

/** 摘要里的分组顺序：先做完的，再在做的，最后还没开始的。 */
const SUMMARY_ORDER: readonly PlanStepStatus[] = ["completed", "in_progress", "pending"];

/** Step marker. The status word is announced separately (see the `sr-only` span). */
function StepGlyph({ status }: { status: PlanStepStatus }) {
  if (status === "completed") return <Check className="plan-panel-glyph" size={13} strokeWidth={2.4} aria-hidden="true" />;
  if (status === "in_progress") return <Loader2 className="plan-panel-glyph spin" size={13} strokeWidth={2} aria-hidden="true" />;
  return <Circle className="plan-panel-glyph" size={12} strokeWidth={1.8} aria-hidden="true" />;
}

/**
 * 输入框上方的任务面板（`update_plan` 的步骤清单）。
 *
 * 计划原本作为一条会话行渲染（`messages/PlanItem`），会随历史滚走；而「这一轮还剩
 * 哪几步」是**当下状态**，不是历史内容，所以放到输入框上方，与 `ApprovalCard`
 * 同一个位置、同一套容器宽度。
 *
 * 什么时候出现由 `lib/plan.ts` 的 `planPanelVisible` 决定（回合结束 / 计划走完 /
 * 用户关掉都不再出现）。`key` 用计划行的 `message_id`：新计划回到默认展开态，
 * 也不继承上一条的关闭与折叠。
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
  const counts: Record<PlanStepStatus, number> = { completed: 0, in_progress: 0, pending: 0 };
  for (const entry of plan.entries) counts[entry.status] += 1;
  // 只列非零分组：全做完时「0 进行中 · 0 待开始」是噪音。
  const summary = SUMMARY_ORDER.filter((key) => counts[key] > 0)
    .map((key) => `${counts[key]} ${t(`activity.planStep.${key}`)}`)
    .join(" · ");

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
        <span className="plan-panel-summary">{summary}</span>
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
    {expanded && <ol className="plan-panel-steps" id={stepsId}>
      {plan.entries.map((entry, index) => (
        <li className={`plan-panel-step is-${entry.status.replace(/_/g, "-")}`} key={`${index}:${entry.content}`}>
          <StepGlyph status={entry.status} />
          {/* 图形是 aria-hidden 的，状态词必须另给读屏，否则「哪几步完成」只有视力用户拿得到。 */}
          <span className="sr-only">{t(`activity.planStep.${entry.status}`)}</span>
          <span className="plan-panel-step-text">{entry.content}</span>
        </li>
      ))}
    </ol>}
  </section>;
}
