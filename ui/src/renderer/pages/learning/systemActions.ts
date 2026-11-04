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
  reason: string;
  at: number;
}

/** 动作行投影：failed = 红 + role=alert + reason；done = 绿；未知 = 中性。 */
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
    reason: failed ? action.reason : '',
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
