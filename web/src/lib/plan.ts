/**
 * 「当前计划是哪一条」——纯派生，与 `lib/approvals.ts` 的 `pendingDecision` 同一角色：
 * 规则留在可单测的函数里，组件只负责画。
 */

import { planData, type PlanData } from "./activity";
import type { ConversationMessage } from "./protocol";

export type CurrentPlan = { plan: PlanData; message: ConversationMessage };

/**
 * 已加载会话里**最后一条能解码的** `plan` 行；没有就是 `null`。
 *
 * 服务端按 `session_id` 就地更新同一条 `plan` 行（`persist_plan`），所以「最后一条」
 * 就是当前这一份计划，不是历史堆叠。解不开的行跳过——调用方据此整块不渲染，而
 * `ActivityItem` 那边仍会把读不懂的旧行按兜底样式显示，不在这里猜形状。
 */
export function latestPlan(messages: ConversationMessage[]): CurrentPlan | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.message_type !== "plan") continue;
    const plan = planData(message.content);
    if (plan) return { plan, message };
  }
  return null;
}

/**
 * 计划面板什么时候该出现 —— 它是**当下工作**的指示器，不是历史记录。
 *
 * 三条收起规则：
 * - **回合结束**（`isProcessing` 转 false）就收起。留着只会显示一份没人再推进的计划
 *   （现场：答复里写着"全部完成"，面板还挂在「1 进行中 · 2 待开始」）。
 * - **计划全部完成**就收起：没有待办，面板没有可说的。
 * - 用户**手动关掉的那一份**按计划行 id 记住（`dismissedId`）；新计划是新 id，
 *   不继承上一次的关闭。
 *
 * 反过来：只有「回合在跑 **且** 还有未完成步骤」才显示。
 */
export function planPanelVisible(
  current: CurrentPlan | null,
  isProcessing: boolean,
  dismissedId: string | null,
): boolean {
  if (!current || !isProcessing) return false;
  if (dismissedId === current.message.message_id) return false;
  return current.plan.entries.some((entry) => entry.status !== "completed");
}
