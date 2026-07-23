/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type GuidTaskIntentId = 'fix-code' | 'summarize' | 'automate' | 'research' | 'freeform';

export type GuidReadinessBlocker = 'model' | 'workspace' | null;

export type GuidExecutionStep = {
  key: string;
  defaultLabel: string;
};

export type GuidTaskIntent = {
  id: GuidTaskIntentId;
  textKey: string;
  defaultText: string;
  requiresWorkspace: boolean;
  expectedArtifactKey: string;
  expectedArtifactDefault: string;
  planSteps: ReadonlyArray<GuidExecutionStep>;
};

/** Shared Day-1 intents for Guid homepage and empty conversation prompts. */
export const GUID_TASK_INTENTS: ReadonlyArray<GuidTaskIntent> = [
  {
    id: 'fix-code',
    textKey: 'guid.taskIntents.fixCode',
    defaultText: '分析这个项目的失败测试，修好后告诉我根因',
    requiresWorkspace: true,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.fixCode',
    expectedArtifactDefault: '修复后的测试结果与根因说明',
    planSteps: [
      { key: 'guid.plan.readProject', defaultLabel: '读取项目' },
      { key: 'guid.plan.locateFailure', defaultLabel: '定位失败' },
      { key: 'guid.plan.fixVerify', defaultLabel: '修改并验证' },
      { key: 'guid.plan.reportRootCause', defaultLabel: '汇报根因' },
    ],
  },
  {
    id: 'summarize',
    textKey: 'guid.taskIntents.summarize',
    defaultText: '阅读当前工作区，用要点总结架构与风险',
    requiresWorkspace: true,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.summarize',
    expectedArtifactDefault: '架构摘要与风险清单',
    planSteps: [
      { key: 'guid.plan.readProject', defaultLabel: '读取项目' },
      { key: 'guid.plan.mapArchitecture', defaultLabel: '梳理架构' },
      { key: 'guid.plan.flagRisks', defaultLabel: '标出风险' },
      { key: 'guid.plan.writeBrief', defaultLabel: '输出摘要' },
    ],
  },
  {
    id: 'automate',
    textKey: 'guid.taskIntents.automate',
    defaultText: '帮我把重复手工步骤整理成可自动执行的流程',
    requiresWorkspace: false,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.automate',
    expectedArtifactDefault: '可执行流程草案',
    planSteps: [
      { key: 'guid.plan.clarifySteps', defaultLabel: '澄清步骤' },
      { key: 'guid.plan.designFlow', defaultLabel: '设计流程' },
      { key: 'guid.plan.draftAutomation', defaultLabel: '起草自动化' },
      { key: 'guid.plan.handoff', defaultLabel: '交付可复用方案' },
    ],
  },
  {
    id: 'research',
    textKey: 'guid.taskIntents.research',
    defaultText: '把这个问题调研清楚，给出结论与下一步建议',
    requiresWorkspace: false,
    expectedArtifactKey: 'guid.taskReceipt.artifacts.research',
    expectedArtifactDefault: '结论与下一步建议',
    planSteps: [
      { key: 'guid.plan.clarifyQuestion', defaultLabel: '澄清问题' },
      { key: 'guid.plan.gatherContext', defaultLabel: '收集上下文' },
      { key: 'guid.plan.synthesize', defaultLabel: '综合结论' },
      { key: 'guid.plan.nextSteps', defaultLabel: '给出下一步' },
    ],
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
  planSteps: ReadonlyArray<GuidExecutionStep>;
};

const FREEFORM_PLAN: ReadonlyArray<GuidExecutionStep> = [
  { key: 'guid.plan.understandGoal', defaultLabel: '理解目标' },
  { key: 'guid.plan.chooseTools', defaultLabel: '选择工具' },
  { key: 'guid.plan.execute', defaultLabel: '执行并检查' },
  { key: 'guid.plan.deliver', defaultLabel: '交付成果' },
];

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
    planSteps: intent?.planSteps ?? FREEFORM_PLAN,
  };
}

export type GuidStatusChip = {
  id: 'project' | 'plan' | 'permission';
  state: 'ready' | 'blocked' | 'optional';
  labelKey: string;
  defaultLabel: string;
};

export function buildGuidStatusChips(input: {
  readiness: GuidReadinessResult;
  hasDraft: boolean;
}): GuidStatusChip[] {
  const { readiness, hasDraft } = input;
  const projectState: GuidStatusChip['state'] =
    readiness.blocker === 'workspace'
      ? 'blocked'
      : readiness.hasWorkspace
        ? 'ready'
        : readiness.requiresWorkspace
          ? 'blocked'
          : 'optional';

  const planState: GuidStatusChip['state'] =
    readiness.blocker === 'model' ? 'blocked' : hasDraft || readiness.intent ? 'ready' : 'optional';

  return [
    {
      id: 'project',
      state: projectState,
      labelKey:
        projectState === 'blocked'
          ? 'guid.status.projectBlocked'
          : projectState === 'ready'
            ? 'guid.status.projectReady'
            : 'guid.status.projectOptional',
      defaultLabel:
        projectState === 'blocked' ? '项目待选择' : projectState === 'ready' ? '项目已识别' : '项目可选',
    },
    {
      id: 'plan',
      state: planState,
      labelKey:
        planState === 'blocked'
          ? 'guid.status.planBlocked'
          : planState === 'ready'
            ? 'guid.status.planReady'
            : 'guid.status.planIdle',
      defaultLabel:
        planState === 'blocked' ? '需连接模型' : planState === 'ready' ? '执行方案已准备' : '等待描述目标',
    },
    {
      id: 'permission',
      state: 'ready',
      labelKey: 'guid.status.permissionAsk',
      defaultLabel: '权限按需询问',
    },
  ];
}

/** Intents shown when a workspace is linked (code/project path). */
export function intentsForWorkspace(hasWorkspace: boolean): ReadonlyArray<GuidTaskIntent> {
  if (hasWorkspace) {
    return GUID_TASK_INTENTS.filter((item) => item.id === 'fix-code' || item.id === 'summarize' || item.id === 'automate');
  }
  return GUID_TASK_INTENTS.filter((item) => item.id === 'research' || item.id === 'automate' || item.id === 'summarize');
}
