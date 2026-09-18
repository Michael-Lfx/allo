import { Check, Circle, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PlanData, PlanStepStatus } from "../lib/activity";
import { planCounts } from "../lib/plan";

/**
 * 计划的**共享呈现**：面板（输入框上方，管「当下」）与会话流里的历史行
 * （`messages/PlanItem`，管「历史」）用的是同一份摘要与同一份步骤清单。
 *
 * 放在这个文件而不是留在 `PlanPanel.tsx` 里，是因为后者已经不是它唯一的家；类名
 * 也相应去掉了 `plan-panel-` 前缀（`plan-steps` / `plan-step` / `plan-glyph`），
 * 缩进这类**容器特有**的样式留给各自的容器去加（`.plan-panel .plan-steps` /
 * `.plan-item .plan-steps`）。
 */

/** 摘要里的分组顺序：先做完的，再在做的，最后还没开始的。 */
const SUMMARY_ORDER: readonly PlanStepStatus[] = ["completed", "in_progress", "pending"];

/** Step marker. The status word is announced separately (see the `sr-only` span). */
function StepGlyph({ status }: { status: PlanStepStatus }) {
  if (status === "completed") return <Check className="plan-glyph" size={13} strokeWidth={2.4} aria-hidden="true" />;
  if (status === "in_progress") return <Loader2 className="plan-glyph spin" size={13} strokeWidth={2} aria-hidden="true" />;
  return <Circle className="plan-glyph" size={12} strokeWidth={1.8} aria-hidden="true" />;
}

/**
 * `1 已完成 · 1 进行中` —— 两处共用一个函数算出来，措辞与报数不会各自漂移。
 * 只列非零分组：全做完时「0 进行中 · 0 待开始」是噪音。
 */
export function PlanSummary({ plan, className }: { plan: PlanData; className?: string }) {
  const { t } = useTranslation();
  const counts = planCounts(plan);
  const text = SUMMARY_ORDER.filter((key) => counts[key] > 0)
    .map((key) => `${counts[key]} ${t(`activity.planStep.${key}`)}`)
    .join(" · ");
  return <span className={className}>{text}</span>;
}

/** 步骤清单。`id` 由调用方给（两个调用点各自 `useId`），供 `aria-controls` 指回。 */
export function PlanSteps({ plan, id }: { plan: PlanData; id: string }) {
  const { t } = useTranslation();
  return <ol className="plan-steps" id={id}>
    {plan.entries.map((entry, index) => (
      <li className={`plan-step is-${entry.status.replace(/_/g, "-")}`} key={`${index}:${entry.content}`}>
        <StepGlyph status={entry.status} />
        {/* 图形是 aria-hidden 的，状态词必须另给读屏，否则「哪几步完成」只有视力用户拿得到。 */}
        <span className="sr-only">{t(`activity.planStep.${entry.status}`)}</span>
        <span className="plan-step-text">{entry.content}</span>
      </li>
    ))}
  </ol>;
}
