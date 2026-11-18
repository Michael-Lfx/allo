/**
 * 题型注册表（pf5 地基票）：每种题型一条 descriptor 的单源表。废弃状态与
 * 能力消费面——生成契约（llmGeneratable）、节完成门槛（countsTowardSection）、
 * 复习抽题（reviewEligible）、逐项反馈（perItemFeedback）——全部从表派生，
 * 未来题型废弃/新增/变更 = 改一条 descriptor 的单点操作。
 *
 * 与后端同源：nomifun-learning models.rs 的 ActivityKind capability 方法
 * （is_deprecated / llm_generatable / review_eligible / counts_toward_section /
 * per_item_feedback）。两端语义对齐各带同源测试钉住，改任一侧须回改另一侧。
 *
 * 存量兼容面不受注册表影响：已作答 matching 的 attempt/XP/打卡（学习账本）、
 * 判分 evaluate Matching 臂、ActivityInput 匹配题渲染分支、复习卡 kind 映射
 * 全部保留——deprecated 只收敛「新生成」与「新抽题」，不动存量事实。
 */
import type { ActivityKind } from './types';

/** 题型两态：active 在役 / deprecated 已废弃（存量兼容保留，消费面派生退场）。 */
export type ActivityKindStatus = 'active' | 'deprecated';

export interface ActivityKindDescriptor {
  status: ActivityKindStatus;
  /** deprecated 题型的接替者。classify（pf2 判别题专有题型）尚未落地——
   * 先以字符串字面量留痕，pf2 落地后收窄为 ActivityKind。 */
  replacedBy?: string;
  /** 进生成契约：LLM 出题面提供该题型。 */
  llmGeneratable: boolean;
  /** 计入节完成门槛：练习轮出题面包含该题型（不计入的行随过滤退场——
   * 全 deprecated 节由既有 `round.length > 0` 守卫天然自动视为完成）。 */
  countsTowardSection: boolean;
  /** 进复习抽题：复习队列出队该题型的卡（存量卡原样保留，只是不再被抽到）。 */
  reviewEligible: boolean;
  /** 作答支持逐项结果（pf1 逐项反馈挂靠点）；reflection/open_question 是
   * AI 批改的整体判分语义，没有逐项结果。 */
  perItemFeedback: boolean;
}

export const ACTIVITY_KIND_REGISTRY: Record<ActivityKind, ActivityKindDescriptor> = {
  single_choice: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  true_false: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  reflection: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: false,
  },
  fill_in_blank: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  multi_choice: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  numeric: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  ordering: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: true,
  },
  /** 首例废弃（pf5）：生成近重复候选 × 逐位置精确比对 × 无逐项反馈的探针
   * 设计缺陷，接替者 classify（pf2）。存量判分/渲染/账本保留，消费面派生退场。 */
  matching: {
    status: 'deprecated',
    replacedBy: 'classify',
    llmGeneratable: false,
    countsTowardSection: false,
    reviewEligible: false,
    perItemFeedback: true,
  },
  open_question: {
    status: 'active',
    llmGeneratable: true,
    countsTowardSection: true,
    reviewEligible: true,
    perItemFeedback: false,
  },
};
