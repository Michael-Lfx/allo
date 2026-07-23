/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type GuidTaskIntentId = 'fix-code' | 'summarize' | 'automate' | 'freeform';

export type GuidReadinessBlocker = 'model' | 'workspace' | null;

export type GuidTaskIntent = {
  id: GuidTaskIntentId;
  textKey: string;
  defaultText: string;
  requiresWorkspace: boolean;
  expectedArtifactKey: string;
  expectedArtifactDefault: string;
};

export const GUID_TASK_INTENTS: ReadonlyArray<GuidTaskIntent> = [
  {
    id: 'fix-code',
    textKey: 'guid.taskIntents.fixCode',
    defaultText: '分析这个项目的失败测试，修好后告诉我根因',
    requiresWorkspace: true,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.fixCode',
    expectedArtifactDefault: '修复后的测试结果与根因说明',
  },
  {
    id: 'summarize',
    textKey: 'guid.taskIntents.summarize',
    defaultText: '阅读当前工作区，用要点总结架构与风险',
    requiresWorkspace: true,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.summarize',
    expectedArtifactDefault: '架构摘要与风险清单',
  },
  {
    id: 'automate',
    textKey: 'guid.taskIntents.automate',
    defaultText: '帮我把重复手工步骤整理成可自动执行的流程',
    requiresWorkspace: false,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.automate',
    expectedArtifactDefault: '可执行流程草案',
  },
] as const;

export type GuidReadinessInput = {
  intentId: GuidTaskIntentId;
  hasModel: boolean;
  workspaceDir?: string | null;
  needsModelForAgent: boolean;
};

export type GuidReadinessResult = {
  intent: GuidTaskIntent | null;
  ready: boolean;
  blocker: GuidReadinessBlocker;
  requiresWorkspace: boolean;
  hasWorkspace: boolean;
  hasModel: boolean;
  primaryAction: 'send' | 'addModel' | 'linkWorkspace';
};

export function resolveGuidIntent(intentId: GuidTaskIntentId): GuidTaskIntent | null {
  if (intentId === 'freeform') return null;
  return GUID_TASK_INTENTS.find((item) => item.id === intentId) ?? null;
}

export function resolveGuidReadiness(input: GuidReadinessInput): GuidReadinessResult {
  const intent = resolveGuidIntent(input.intentId);
  const requiresWorkspace = intent?.requiresWorkspace ?? false;
  const hasWorkspace = Boolean(input.workspaceDir?.trim());
  const hasModel = input.needsModelForAgent ? input.hasModel : true;

  let blocker: GuidReadinessBlocker = null;
  if (!hasModel) blocker = 'model';
  else if (requiresWorkspace && !hasWorkspace) blocker = 'workspace';

  const primaryAction =
    blocker === 'model' ? 'addModel' : blocker === 'workspace' ? 'linkWorkspace' : 'send';

  return {
    intent,
    ready: blocker === null,
    blocker,
    requiresWorkspace,
    hasWorkspace,
    hasModel,
    primaryAction,
  };
}

export type GuidTaskReceipt = {
  goal: string;
  context: 'workspace' | 'none';
  expectedArtifactKey: string;
  expectedArtifactDefault: string;
};

export function buildGuidTaskReceipt(
  intentId: GuidTaskIntentId,
  goalText: string,
  workspaceDir?: string | null
): GuidTaskReceipt | null {
  const intent = resolveGuidIntent(intentId);
  if (!intent && !goalText.trim()) return null;
  return {
    goal: goalText.trim() || intent?.defaultText || '',
    context: workspaceDir?.trim() ? 'workspace' : 'none',
    expectedArtifactKey: intent?.expectedArtifactKey ?? 'guid.taskReceipt.artifacts.freeform',
    expectedArtifactDefault: intent?.expectedArtifactDefault ?? '可检查的任务成果',
  };
}
