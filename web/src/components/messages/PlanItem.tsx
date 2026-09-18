import { useId } from "react";
import { ChevronRight, ListChecks } from "lucide-react";
import { useTranslation } from "react-i18next";
import { planData } from "../../lib/activity";
import { PlanSteps, PlanSummary } from "../PlanSteps";
import type { ConversationMessage } from "../../lib/protocol";

/**
 * 会话流里的计划行 —— 这份计划的**历史**，与输入框上方的面板（**当下**）互补。
 *
 * 面板在回合结束后收起（`planPanelVisible`），于是「这一轮原本打算做什么、停在哪一步」
 * 就只剩这里能查。这正是它必须存在的理由：**中断 / 放弃**的回合有未完成步骤、却已经
 * 不在处理中，那是最需要痕迹的时候。
 *
 * 它天然是**每份计划一行**：服务端按 `plan_id` 就地更新同一条记录
 * （`stream_relay::persist_plan`），所以这里看到的是那份计划的最终状态，而不是每次
 * `update_plan` 都留一行——后者正是当初把计划从流里拿掉的原因（一轮能刷十几行）。
 *
 * 用原生 `<details>`：与 `ToolCallItem` / `ThinkingItem` 同一套折叠范式，可访问性免费。
 * 默认**收起**——历史不该抢版面；要逐步细看再展开。
 */
export function PlanItem({ message }: { message: ConversationMessage }) {
  const { t } = useTranslation();
  const stepsId = useId();
  const plan = planData(message.content);
  // 解不开的行整块不渲染：`ActivityItem` 那边对读不懂的旧行有兜底样式，这里不猜形状。
  if (!plan) return null;
  return <details className="plan-item">
    <summary>
      <span className="plan-item-icon" aria-hidden="true"><ListChecks size={14} strokeWidth={1.7} /></span>
      <span className="plan-item-title">{t("activity.plan")}</span>
      <PlanSummary plan={plan} className="plan-item-summary" />
      <ChevronRight className="plan-item-caret" aria-hidden="true" size={15} strokeWidth={1.7} />
    </summary>
    <PlanSteps plan={plan} id={stepsId} />
  </details>;
}
