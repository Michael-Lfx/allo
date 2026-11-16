/** 系统动作簿的展示投影（y04）。纯函数：机器名 → i18n key / 徽标色 /
 * role=alert 决策。组件只负责排版（与 autoRefill.ts 同款「决策可测」纪律）。 */
import type { SystemActionView } from './types';

/** kind / trigger / result 的机器名 → i18n key 单源（未知值走宽容兜底）。 */
export const typeLabelKeys = {
  kind: {
    maintenance_repair: 'learning.learningGraphSystemActionKindRepair',
    maintenance_mount: 'learning.learningGraphSystemActionKindMount',
    growth: 'learning.learningGraphSystemActionKindGrowth',
    unknown: 'learning.learningGraphSystemActionKindUnknown',
  },
  trigger: {
    timer: 'learning.learningGraphSystemActionTriggerTimer',
    manual: 'learning.learningGraphSystemActionTriggerManual',
    settlement: 'learning.learningGraphSystemActionTriggerSettlement',
    auto_refill: 'learning.learningGraphSystemActionTriggerAutoRefill',
    unknown: 'learning.learningGraphSystemActionTriggerUnknown',
  },
  result: {
    done: 'learning.learningGraphSystemActionResultDone',
    failed: 'learning.learningGraphSystemActionResultFailed',
    unknown: 'learning.learningGraphSystemActionResultUnknown',
  },
} as const;

const kindKeys: Record<string, string> = typeLabelKeys.kind;
const triggerKeys: Record<string, string> = typeLabelKeys.trigger;
const resultKeys: Record<string, string> = typeLabelKeys.result;

function labelKey(map: Record<string, string>, machineName: string, unknownKey: string): string {
  return map[machineName] ?? unknownKey;
}

/**
 * 失败原因分类（o1）：后端 reason 是 AppError 诊断原文，直出会把契约门报文
 * 和机器文本摆上人面。这里按稳定错误前缀归类成词典 key，无匹配落中性
 * 「未知原因」；分类句是人面主显示，原文经 `reasonDetail` 收进 Tooltip
 * （只收纳不删信息）。
 *
 * 匹配的后端文案清单（`nomifun-common` `error.rs` 的 AppError Display 串）：
 * - `Conflict: …` / `… already running`        → 冲突
 * - `Request timeout: …`                       → 超时
 * - `Unprocessable entity: …`                  → 产物未过校验
 * - `Not implemented: …`                       → 暂不支持（能力未落地是中性
 *   事实，不是产物没过校验）
 * - `Rate limited` / `Bad gateway: …` /
 *   `No usable model provider is configured: …` → 繁忙
 * - `Internal error: …`                        → 服务暂时不可用（服务端故障，
 *   不是排队繁忙）
 *
 * 顺序即优先级：Display 串以最外层变体开头，`Bad gateway: Agent internal
 * error …` 这类嵌套报文按最外层归繁忙档，所以繁忙档排在 internal error 前。
 *
 * 已知脆弱面：这是对后端英文措辞的正则匹配，没有结构化错误码可依——后端
 * 改措辞或新增变体时，对应行会**静默降级为「未知原因」**（不报错、不误归
 * 类，原文仍可在 Tooltip 里看到）。后端动 AppError Display 串时需同步此表。
 */
const REASON_MATCHERS: ReadonlyArray<readonly [RegExp, string]> = [
  [/conflict|already running/i, 'learning.systemActionReasonConflict'],
  [/timeout/i, 'learning.systemActionReasonTimeout'],
  [/unprocessable entity/i, 'learning.systemActionReasonInvalidOutput'],
  [/not implemented/i, 'learning.systemActionReasonUnsupported'],
  [/rate limited|bad gateway|provider unavailable|no usable model/i, 'learning.systemActionReasonBusy'],
  [/internal error/i, 'learning.systemActionReasonServiceUnavailable'],
];

/** reason 原文 → 分类词典 key；空串/无匹配一律「未知原因」。导出供测试。 */
export function systemActionReasonKey(reason: string): string {
  const trimmed = reason.trim();
  if (trimmed === '') return 'learning.systemActionReasonUnknown';
  for (const [pattern, key] of REASON_MATCHERS) {
    if (pattern.test(trimmed)) return key;
  }
  return 'learning.systemActionReasonUnknown';
}

/** 一条动作行的展示模型。 */
export interface SystemActionRowModel {
  key: string;
  kindKey: string;
  triggerKey: string;
  basis: string;
  resultKey: string;
  resultColor: 'green' | 'red' | 'gray';
  /** 失败行 = 'alert'（读屏播报）；done/未知 = undefined。 */
  resultRole: 'alert' | undefined;
  /** 失败原因的分类词典 key（原文不直出）；done 行 = ''（不渲染）。 */
  reasonKey: string;
  /** 失败原因原文（诊断面）：组件侧只经 Tooltip 收纳（悬停可见），绝不作为
   * 主显示文本直出。空 = 无原文可收纳（不出 Tooltip）；done 行 = ''。 */
  reasonDetail: string;
  at: number;
}

/** 动作行投影：failed = 红 + role=alert + reason 分类映射；done = 绿；未知 = 中性。 */
export function buildSystemActionRow(action: SystemActionView): SystemActionRowModel {
  const failed = action.result === 'failed';
  const done = action.result === 'done';
  return {
    key: action.action_id,
    kindKey: labelKey(kindKeys, action.kind, typeLabelKeys.kind.unknown),
    triggerKey: labelKey(triggerKeys, action.trigger, typeLabelKeys.trigger.unknown),
    basis: action.basis,
    resultKey: labelKey(resultKeys, action.result, typeLabelKeys.result.unknown),
    resultColor: failed ? 'red' : done ? 'green' : 'gray',
    resultRole: failed ? 'alert' : undefined,
    reasonKey: failed ? systemActionReasonKey(action.reason) : '',
    reasonDetail: failed ? action.reason.trim() : '',
    at: action.at,
  };
}

/** 列表投影：保留后端倒序；空列表 = 空态。 */
export function buildSystemActionRows(actions: SystemActionView[]): {
  rows: SystemActionRowModel[];
  empty: boolean;
} {
  return { rows: actions.map(buildSystemActionRow), empty: actions.length === 0 };
}

/** 批行状态 tag（applied 无 tag）：pending 进行中、failed 红、cancelled 中性灰。 */
export interface BatchStatusMeta {
  labelKey: string;
  color: 'processing' | 'red' | 'gray';
  /** 中性终态（cancelled）：不是失败，不染红。 */
  neutral: boolean;
}

const BATCH_STATUS_META: Record<string, BatchStatusMeta> = {
  pending: {
    labelKey: 'learning.learningGraphBatchPending',
    color: 'processing',
    neutral: false,
  },
  failed: {
    labelKey: 'learning.learningGraphBatchFailed',
    color: 'red',
    neutral: false,
  },
  cancelled: {
    labelKey: 'learning.learningGraphBatchCancelled',
    color: 'gray',
    neutral: true,
  },
};

export function batchStatusMeta(status: string): BatchStatusMeta | undefined {
  return BATCH_STATUS_META[status];
}
