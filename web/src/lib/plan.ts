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
