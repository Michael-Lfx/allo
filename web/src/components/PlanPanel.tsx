import { useId, useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, Circle, ListChecks, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { latestPlan } from "../lib/plan";
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
 * 输入框上方的常驻任务面板（`update_plan` 的步骤清单）。
 *
 * 计划原本作为一条会话行渲染（`messages/PlanItem`），会随历史滚走；而「这一轮还剩
 * 哪几步」是**当下状态**，不是历史内容，所以放到输入框上方常驻，与 `ApprovalCard`
 * 同一个位置、同一套容器宽度。
 *
 * `key` 用计划行的 `message_id`：新计划回到默认展开态，不继承上一条计划的折叠选择。
 */
export function PlanPanel() {
  const messages = useAppStore((s) => s.stream.messages);
  const current = useMemo(() => latestPlan(messages), [messages]);
  if (!current) return null;
  return <PlanPanelCard key={current.message.message_id} plan={current.plan} />;
}

/** 面板本体：只认 `plan`，取数由 `PlanPanel` 负责。 */
export function PlanPanelCard({ plan }: { plan: PlanData }) {
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
    <button
      type="button"
      className="plan-panel-head"
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
