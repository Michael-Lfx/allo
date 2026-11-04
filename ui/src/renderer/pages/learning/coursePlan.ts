/** 建课计划态（y05）：填写 → 计划 → 生成的纯状态机与读数模型。
 *
 * 三个入口（知识库 / 描述 / 学习图）在校验通过后只构造请求并进入计划态；
 * 计划态期间发送的只有一次性、零 LLM 的知识库规模估算（描述/图流完全不
 * 发估算），生成请求只由「开始生成」产生且每个计划恰好一次。返回修改与
 * 关闭都是零请求；估算按请求键缓存，重进同一计划不重复预取。
 *
 * 全部逻辑零渲染、零副作用——hook 只负责执行这里返回的 effects。 */
import type {
  CourseGenerationEstimateView,
  GenerateCourseRequest,
  TeachingStyle,
} from './types';
import { estimateLines, triageCountsLine, triageViewCounts, uncertainTooMany } from './model';
import type { Translate } from './utils';

export type CreationTab = 'base' | 'description' | 'graph';

/** 对话框阶段：form（表单）/ plan（计划确认）/ generating（现有进度视图，
 * 含 completed/failed/cancelled 终态）。 */
export type CreationPhase = 'form' | 'plan' | 'generating';

/** 表单快照：三个入口共用的输入面（hook 从各自 state 组装）。 */
export interface CourseCreationForm {
  tab: CreationTab;
  goal: string;
  description: string;
  knowledgeBaseIds: string[];
  domain: string;
  teachingStyle: TeachingStyle;
  providerId?: string;
  model?: string;
  graphProficiency?: string;
  graphBasis?: string;
}

type PlanValidationKey =
  | 'learning.form.modelPending'
  | 'learning.selectKnowledgeBase'
  | 'learning.goalRequired'
  | 'learning.kbSelectionTooMany'
  | 'learning.describeRequired';

export type PlanBuildResult =
  | { kind: 'invalid'; messageKey: PlanValidationKey; messageVars?: { count: number } }
  | { kind: 'ready'; request: GenerateCourseRequest };

/** 构造提交请求（计划态的入参）：校验失败只回 i18n 键，不产生任何请求。
 * 上限由调用方传入（与后端 `COURSE_MAX_KNOWLEDGE_BASES` 同一口径）。 */
export function buildPlanRequest(
  form: CourseCreationForm,
  maxKnowledgeBases: number
): PlanBuildResult {
  if (!form.providerId || !form.model) {
    return { kind: 'invalid', messageKey: 'learning.form.modelPending' };
  }
  const modelFields = { provider_id: form.providerId, model: form.model };
  if (form.tab === 'base') {
    if (form.knowledgeBaseIds.length === 0) {
      return { kind: 'invalid', messageKey: 'learning.selectKnowledgeBase' };
    }
    const goal = form.goal.trim();
    if (!goal) {
      return { kind: 'invalid', messageKey: 'learning.goalRequired' };
    }
    if (form.knowledgeBaseIds.length > maxKnowledgeBases) {
      return {
        kind: 'invalid',
        messageKey: 'learning.kbSelectionTooMany',
        messageVars: { count: maxKnowledgeBases },
      };
    }
    const domain = form.domain.trim();
    return {
      kind: 'ready',
      request: {
        knowledge_base_ids: [...form.knowledgeBaseIds],
        goal,
        ...(domain ? { domain } : {}),
        teaching_style: form.teachingStyle,
        ...modelFields,
      },
    };
  }
  const description = form.description.trim();
  if (!description) {
    return { kind: 'invalid', messageKey: 'learning.describeRequired' };
  }
  if (form.tab === 'graph') {
    const proficiency = form.graphProficiency?.trim() ?? '';
    const basis = form.graphBasis?.trim() ?? '';
    return {
      kind: 'ready',
      request: {
        course_kind: 'learning_graph',
        description,
        ...(proficiency ? { proficiency_statement: proficiency } : {}),
        ...(basis ? { learner_basis: basis } : {}),
        teaching_style: form.teachingStyle,
        ...modelFields,
      },
    };
  }
  return {
    kind: 'ready',
    request: {
      description,
      teaching_style: form.teachingStyle,
      ...modelFields,
    },
  };
}

/** 当前计划：tab + 原样请求 + 估算缓存键（仅 KB 流）。 */
export interface CoursePlan {
  tab: CreationTab;
  request: GenerateCourseRequest;
  estimateKey: string | null;
}

export type PlanEstimate =
  | { status: 'loading' }
  | { status: 'failed' }
  | { status: 'ready'; estimate: CourseGenerationEstimateView };

export interface CoursePlanState {
  plan: CoursePlan | null;
  /** 估算缓存（键 = planEstimateKey）：重进相同请求不重复预取 */
  estimates: Record<string, PlanEstimate>;
}

export const INITIAL_PLAN_STATE: CoursePlanState = { plan: null, estimates: {} };

export type PlanEffect =
  | { type: 'estimate'; key: string; request: GenerateCourseRequest }
  | { type: 'generate'; request: GenerateCourseRequest };

export type PlanAction =
  | { type: 'submit'; tab: CreationTab; request: GenerateCourseRequest }
  | { type: 'estimate-loaded'; key: string; estimate: CourseGenerationEstimateView }
  | { type: 'estimate-failed'; key: string }
  | { type: 'confirm' }
  | { type: 'back' };

export interface PlanTransition {
  state: CoursePlanState;
  effects: PlanEffect[];
}

/** 估算只依赖来源库与目标（后端规则分诊的输入）；键保持主来源顺序。 */
export function planEstimateKey(request: GenerateCourseRequest): string {
  const ids =
    request.knowledge_base_ids ??
    (request.knowledge_base_id ? [request.knowledge_base_id] : []);
  return JSON.stringify({ knowledge_base_ids: ids, goal: request.goal ?? '' });
}

/** 计划态状态转移。effects 是唯一允许发起网络请求的出口：
 * submit 最多发一次估算（KB 流），confirm 至多发一次生成（消费计划）。 */
export function reducePlan(state: CoursePlanState, action: PlanAction): PlanTransition {
  switch (action.type) {
    case 'submit': {
      const estimateKey = action.tab === 'base' ? planEstimateKey(action.request) : null;
      const effects: PlanEffect[] = [];
      let estimates = state.estimates;
      if (estimateKey !== null && estimates[estimateKey] === undefined) {
        estimates = { ...estimates, [estimateKey]: { status: 'loading' } };
        effects.push({ type: 'estimate', key: estimateKey, request: action.request });
      }
      return {
        state: {
          plan: { tab: action.tab, request: action.request, estimateKey },
          estimates,
        },
        effects,
      };
    }
    case 'estimate-loaded':
      return {
        state: {
          ...state,
          estimates: {
            ...state.estimates,
            [action.key]: { status: 'ready', estimate: action.estimate },
          },
        },
        effects: [],
      };
    case 'estimate-failed':
      return {
        state: {
          ...state,
          estimates: { ...state.estimates, [action.key]: { status: 'failed' } },
        },
        effects: [],
      };
    case 'confirm': {
      if (!state.plan) return { state, effects: [] };
      // 消费计划：同一计划重复确认不会再发一次生成。
      return {
        state: { ...state, plan: null },
        effects: [{ type: 'generate', request: state.plan.request }],
      };
    }
    case 'back':
      return {
        state: state.plan ? { ...state, plan: null } : state,
        effects: [],
      };
  }
}

/** 计划卡读数来源：KB 流读缓存；描述/图流恒为「不适用」（不显示数字）。 */
export type PlanEstimateView =
  | { kind: 'not_applicable' }
  | { kind: 'loading' }
  | { kind: 'failed' }
  | { kind: 'ready'; estimate: CourseGenerationEstimateView };

export function planEstimateView(
  state: CoursePlanState,
  plan: CoursePlan | null
): PlanEstimateView {
  if (!plan || plan.tab !== 'base' || plan.estimateKey === null) {
    return { kind: 'not_applicable' };
  }
  const entry = state.estimates[plan.estimateKey];
  if (!entry || entry.status === 'loading') return { kind: 'loading' };
  if (entry.status === 'failed') return { kind: 'failed' };
  return { kind: 'ready', estimate: entry.estimate };
}

export function teachingStyleLabel(style: TeachingStyle | undefined, t: Translate): string {
  switch (style) {
    case 'socratic':
      return t('learning.styleSocratic');
    case 'feynman':
      return t('learning.styleFeynman');
    case 'standard':
    default:
      return t('learning.styleStandard');
  }
}

export interface PlanSlotLine {
  key: 'source' | 'target' | 'style' | 'model';
  label: string;
  value: string;
}

/** 四槽回显：方式/来源、目标或简报、教学风格、模型；空值如实「未指定」。 */
export function planSlotLines(
  plan: CoursePlan,
  sources: string[],
  t: Translate
): PlanSlotLine[] {
  const request = plan.request;
  let sourceValue: string;
  if (plan.tab === 'base') {
    sourceValue =
      sources.length > 0
        ? sources.join(t('learning.listSeparator'))
        : t('learning.planUnspecified');
  } else if (plan.tab === 'graph') {
    sourceValue = t('learning.learningGraphTabTitle');
  } else {
    sourceValue = t('learning.generateFromDescription');
  }
  const target = (request.goal ?? request.description ?? '').trim();
  const model = request.model?.trim() ?? '';
  return [
    { key: 'source', label: t('learning.planSlotSource'), value: sourceValue },
    {
      key: 'target',
      label: t('learning.planSlotTarget'),
      value: target || t('learning.planUnspecified'),
    },
    {
      key: 'style',
      label: t('learning.planSlotStyle'),
      value: teachingStyleLabel(request.teaching_style, t),
    },
    {
      key: 'model',
      label: t('learning.planSlotModel'),
      value: model || t('learning.planUnspecified'),
    },
  ];
}

export interface PlanDeclarationLine {
  label: string;
  present: boolean;
}

export interface PlanReadout {
  title: string;
  /** 不可得/估算中/酌定说明；ready 时为 null */
  note: string | null;
  /** 确定性数字行（仅 KB ready；其余恒空，绝不虚构） */
  lines: string[];
  triageLine: string | null;
  degraded: boolean;
  uncertainLine: string | null;
  skippedTitle: string | null;
  skipped: { path: string; reason: string }[];
  /** 超阈值提示：计划卡即成本门，确认按钮就是放行 */
  needsConfirmHint: string | null;
  /** 图流的声明在场情况（描述/KB 流为空） */
  declarations: PlanDeclarationLine[];
}

/** 计划卡读数：KB 用现有零 LLM 估算；图/描述如实「按声明/简报酌定」。 */
export function planReadout(
  plan: CoursePlan,
  view: PlanEstimateView,
  t: Translate
): PlanReadout {
  const base: PlanReadout = {
    title: t('learning.planReadoutTitle'),
    note: null,
    lines: [],
    triageLine: null,
    degraded: false,
    uncertainLine: null,
    skippedTitle: null,
    skipped: [],
    needsConfirmHint: null,
    declarations: [],
  };
  if (plan.tab === 'graph') {
    return {
      ...base,
      note: t('learning.planEstimateByDeclaration'),
      declarations: [
        {
          label: t('learning.learningGraphProficiencyLabel'),
          present: (plan.request.proficiency_statement ?? '').trim().length > 0,
        },
        {
          label: t('learning.learningGraphBasisLabel'),
          present: (plan.request.learner_basis ?? '').trim().length > 0,
        },
      ],
    };
  }
  if (plan.tab === 'description') {
    return { ...base, note: t('learning.planEstimateByBrief') };
  }
  if (view.kind === 'loading') {
    return { ...base, note: t('learning.planEstimating') };
  }
  if (view.kind === 'failed') {
    return { ...base, note: t('learning.planEstimateUnavailable') };
  }
  if (view.kind === 'not_applicable') {
    return base;
  }
  const estimate = view.estimate;
  const counts = triageViewCounts(estimate.triage);
  const skipped = estimate.skipped_examples ?? [];
  return {
    ...base,
    lines: estimateLines(estimate, t, { duration: 'seconds' }),
    triageLine: triageCountsLine(counts, t),
    degraded: estimate.triage.degraded,
    uncertainLine: uncertainTooMany(counts)
      ? t('learning.genUncertainTooMany', { count: counts.uncertain })
      : null,
    skippedTitle: skipped.length > 0 ? t('learning.genTriageExcludedTitle') : null,
    skipped: skipped.map((entry) => ({ path: entry.path, reason: entry.reason })),
    needsConfirmHint: estimate.needs_confirm ? t('learning.planNeedsConfirm') : null,
  };
}
