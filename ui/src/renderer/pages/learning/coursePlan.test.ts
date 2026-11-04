/** 建课计划态（y05）的纯状态机单测：三入口统一 form → plan → generating，
 * 计划态零生成调用、确认恰一次、返回/关闭零请求、KB 估算预取与失败降级、
 * 图/描述流不发估算也不虚构数字。零渲染——组件只消费这里产出的读数。 */
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  buildPlanRequest,
  INITIAL_PLAN_STATE,
  planEstimateView,
  planReadout,
  planSlotLines,
  reducePlan,
  type CourseCreationForm,
  type CoursePlan,
  type CoursePlanState,
} from './coursePlan';
import type { Translate } from './utils';
import type { CourseGenerationEstimateView, GenerateCourseRequest } from './types';

const t = ((key: string, vars?: Record<string, unknown>) => {
  if (key === 'learning.listSeparator') return '、';
  return vars && Object.keys(vars).length > 0 ? `${key}:${JSON.stringify(vars)}` : key;
}) as unknown as Translate;

const form = (overrides: Partial<CourseCreationForm> = {}): CourseCreationForm => ({
  tab: 'base',
  goal: '掌握 Rust 异步编程',
  description: '想系统学会 Rust 异步',
  knowledgeBaseIds: ['kb-1', 'kb-2'],
  domain: '工程',
  teachingStyle: 'socratic',
  providerId: 'provider-a',
  model: 'model-a',
  graphProficiency: '',
  graphBasis: '',
  ...overrides,
});

const estimate = (
  overrides: Partial<CourseGenerationEstimateView> = {}
): CourseGenerationEstimateView => ({
  total_files: 12,
  estimated_batches: 3,
  estimated_llm_calls: 5,
  estimated_seconds: 300,
  needs_confirm: false,
  triage: {
    total_files: 12,
    core: 10,
    prerequisite: 0,
    practice: 0,
    mixed: 1,
    noise: 1,
    duplicate: 0,
    uncertain: 0,
    degraded: true,
  },
  skipped_examples: [],
  ...overrides,
});

const requestFor = (overrides: Partial<GenerateCourseRequest> = {}): GenerateCourseRequest => ({
  knowledge_base_ids: ['kb-1'],
  goal: '掌握 Rust',
  teaching_style: 'standard',
  provider_id: 'provider-a',
  model: 'model-a',
  ...overrides,
});

const planFor = (tab: CoursePlan['tab'], request: GenerateCourseRequest): CoursePlan => ({
  tab,
  request,
  estimateKey: tab === 'base' ? 'kb-key-1' : null,
});

/** 模拟 hook 执行 effect：只记录发出的请求，不发真网络请求。 */
const run = (
  state: CoursePlanState,
  action: Parameters<typeof reducePlan>[1]
): { state: CoursePlanState; effects: ReturnType<typeof reducePlan>['effects'] } => reducePlan(state, action);

const generateEffects = (effects: ReturnType<typeof reducePlan>['effects']) =>
  effects.filter((effect) => effect.type === 'generate');

describe('buildPlanRequest（三入口共用构造）', () => {
  test('知识库流：目标、多库、领域、风格、模型逐字段进请求', () => {
    const result = buildPlanRequest(form(), 5);
    expect(result).toEqual({
      kind: 'ready',
      request: {
        knowledge_base_ids: ['kb-1', 'kb-2'],
        goal: '掌握 Rust 异步编程',
        domain: '工程',
        teaching_style: 'socratic',
        provider_id: 'provider-a',
        model: 'model-a',
      },
    });
  });

  test('描述流：简报原样进请求，不带知识库字段', () => {
    const result = buildPlanRequest(form({ tab: 'description' }), 5);
    expect(result).toEqual({
      kind: 'ready',
      request: {
        description: '想系统学会 Rust 异步',
        teaching_style: 'socratic',
        provider_id: 'provider-a',
        model: 'model-a',
      },
    });
  });

  test('学习图流：目标 + 可选程度/基础声明，course_kind 明确', () => {
    const result = buildPlanRequest(
      form({ tab: 'graph', graphProficiency: '能独立写服务', graphBasis: '会基本语法' }),
      5
    );
    expect(result).toEqual({
      kind: 'ready',
      request: {
        course_kind: 'learning_graph',
        description: '想系统学会 Rust 异步',
        proficiency_statement: '能独立写服务',
        learner_basis: '会基本语法',
        teaching_style: 'socratic',
        provider_id: 'provider-a',
        model: 'model-a',
      },
    });
  });

  test('空声明不落字段（缺省由结构生成扩写 / 按纯零基础画地形）', () => {
    const result = buildPlanRequest(form({ tab: 'graph' }), 5);
    expect(result.kind).toBe('ready');
    if (result.kind !== 'ready') return;
    expect('proficiency_statement' in result.request).toBe(false);
    expect('learner_basis' in result.request).toBe(false);
  });

  test('校验失败只返回 i18n 键，不构造请求', () => {
    expect(buildPlanRequest(form({ goal: '  ' }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.goalRequired',
    });
    expect(buildPlanRequest(form({ knowledgeBaseIds: [] }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.selectKnowledgeBase',
    });
    expect(buildPlanRequest(form({ knowledgeBaseIds: ['a', 'b', 'c', 'd', 'e', 'f'] }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.kbSelectionTooMany',
      messageVars: { count: 5 },
    });
    expect(buildPlanRequest(form({ tab: 'description', description: ' ' }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.describeRequired',
    });
    expect(buildPlanRequest(form({ tab: 'graph', description: ' ' }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.describeRequired',
    });
    expect(buildPlanRequest(form({ tab: 'graph', providerId: undefined, model: undefined }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.form.modelPending',
    });
  });
});

describe('计划态状态机：提交只进计划，不触发生成', () => {
  test('三入口提交后都进入 plan，且零 generate effect', () => {
    for (const tab of ['base', 'description', 'graph'] as const) {
      const request =
        tab === 'base'
          ? requestFor()
          : tab === 'description'
            ? requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '简报' })
            : requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '目标', course_kind: 'learning_graph' });
      const { state, effects } = run(INITIAL_PLAN_STATE, { type: 'submit', tab, request });
      expect(state.plan?.tab).toBe(tab);
      expect(state.plan?.request).toEqual(request);
      expect(generateEffects(effects)).toEqual([]);
    }
  });

  test('KB 流进入计划时预取一次估算；描述/图流一次估算请求都不发', () => {
    const base = run(INITIAL_PLAN_STATE, { type: 'submit', tab: 'base', request: requestFor() });
    expect(base.effects).toEqual([
      { type: 'estimate', key: expect.any(String), request: requestFor() },
    ]);

    const description = run(INITIAL_PLAN_STATE, {
      type: 'submit',
      tab: 'description',
      request: requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '简报' }),
    });
    expect(description.effects).toEqual([]);

    const graph = run(INITIAL_PLAN_STATE, {
      type: 'submit',
      tab: 'graph',
      request: requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '目标', course_kind: 'learning_graph' }),
    });
    expect(graph.effects).toEqual([]);
  });

  test('「开始生成」恰调用一次：确认消费计划，重复确认零 effect', () => {
    const entered = run(INITIAL_PLAN_STATE, { type: 'submit', tab: 'description', request: requestFor({ description: '简报', knowledge_base_ids: undefined, goal: undefined }) });
    const confirmed = run(entered.state, { type: 'confirm' });
    expect(generateEffects(confirmed.effects)).toEqual([
      { type: 'generate', request: entered.state.plan?.request },
    ]);
    expect(confirmed.state.plan).toBeNull();

    const again = run(confirmed.state, { type: 'confirm' });
    expect(again.effects).toEqual([]);
  });

  test('「返回修改」零请求，估算缓存保留；同请求再进计划不重复预取', () => {
    const entered = run(INITIAL_PLAN_STATE, { type: 'submit', tab: 'base', request: requestFor() });
    const key = (entered.effects[0] as { key: string }).key;
    const loaded = run(entered.state, {
      type: 'estimate-loaded',
      key,
      estimate: estimate(),
    });
    const backed = run(loaded.state, { type: 'back' });
    expect(backed.state.plan).toBeNull();
    expect(backed.state.estimates[key]).toEqual({ status: 'ready', estimate: estimate() });

    const reentered = run(backed.state, { type: 'submit', tab: 'base', request: requestFor() });
    expect(reentered.effects).toEqual([]);
    expect(planEstimateView(reentered.state, reentered.state.plan)).toEqual({
      kind: 'ready',
      estimate: estimate(),
    });
  });

  test('估算失败仍可确认（fail-open），且计划卡如实呈现不可得', () => {
    const entered = run(INITIAL_PLAN_STATE, { type: 'submit', tab: 'base', request: requestFor() });
    const key = (entered.effects[0] as { key: string }).key;
    const failed = run(entered.state, { type: 'estimate-failed', key });
    const view = planEstimateView(failed.state, failed.state.plan);
    expect(view).toEqual({ kind: 'failed' });

    const readout = planReadout(failed.state.plan as CoursePlan, view, t);
    expect(readout.note).toBe('learning.planEstimateUnavailable');
    expect(readout.lines).toEqual([]);

    const confirmed = run(failed.state, { type: 'confirm' });
    expect(generateEffects(confirmed.effects)).toHaveLength(1);
  });

  test('KB needs_confirm 在计划读数里显式提示，确认即成本门（无需二次弹窗）', () => {
    const entered = run(INITIAL_PLAN_STATE, { type: 'submit', tab: 'base', request: requestFor() });
    const key = (entered.effects[0] as { key: string }).key;
    const loaded = run(entered.state, {
      type: 'estimate-loaded',
      key,
      estimate: estimate({ needs_confirm: true }),
    });
    const view = planEstimateView(loaded.state, loaded.state.plan);
    const readout = planReadout(loaded.state.plan as CoursePlan, view, t);
    expect(readout.needsConfirmHint).toBe('learning.planNeedsConfirm');
    expect(readout.lines).toEqual([
      'learning.costGateFiles:{"count":12}',
      'learning.costGateBatches:{"count":3}',
      'learning.costGateCalls:{"count":5}',
      'learning.planEstimateSeconds:{"seconds":300}',
    ]);
    expect(readout.triageLine).toBe(
      'learning.genTriageLine:{"total":12,"core":10,"mixed":1,"skipped":1,"duplicate":0,"uncertain":0}'
    );
  });
});

describe('计划态读数：图/描述如实「酌定」，不编数字', () => {
  test('描述流：按简报酌定，零数字行', () => {
    const plan = planFor('description', requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '简报' }));
    const readout = planReadout(plan, { kind: 'not_applicable' }, t);
    expect(readout.note).toBe('learning.planEstimateByBrief');
    expect(readout.lines).toEqual([]);
    expect(readout.declarations).toEqual([]);
  });

  test('学习图流：按声明酌定 + 程度/基础声明是否在场，零数字行', () => {
    const plan = planFor('graph', requestFor({ knowledge_base_ids: undefined, goal: undefined, description: '目标', course_kind: 'learning_graph', proficiency_statement: '能独立写服务' }));
    const readout = planReadout(plan, { kind: 'not_applicable' }, t);
    expect(readout.note).toBe('learning.planEstimateByDeclaration');
    expect(readout.lines).toEqual([]);
    expect(readout.declarations).toEqual([
      { label: 'learning.learningGraphProficiencyLabel', present: true },
      { label: 'learning.learningGraphBasisLabel', present: false },
    ]);
  });

  test('四槽回显：来源/目标/风格/模型；空值显示未指定', () => {
    const plan = planFor('base', requestFor({ goal: '  掌握 Rust  ' }));
    const slots = planSlotLines(plan, ['金融笔记', '算法笔记'], t);
    expect(slots).toEqual([
      { key: 'source', label: 'learning.planSlotSource', value: '金融笔记、算法笔记' },
      { key: 'target', label: 'learning.planSlotTarget', value: '掌握 Rust' },
      { key: 'style', label: 'learning.planSlotStyle', value: 'learning.styleStandard' },
      { key: 'model', label: 'learning.planSlotModel', value: 'model-a' },
    ]);

    const empty = planFor('description', requestFor({ description: '  ', knowledge_base_ids: undefined, goal: undefined, provider_id: undefined, model: undefined }));
    const emptySlots = planSlotLines(empty, [], t);
    expect(emptySlots.map((slot) => slot.value)).toEqual([
      'learning.generateFromDescription',
      'learning.planUnspecified',
      'learning.styleStandard',
      'learning.planUnspecified',
    ]);
  });
});

describe('hook 接线纪律：生成/估算只在计划态效果里发生', () => {
  const hookSource = readFileSync(new URL('./hooks/useCourseCreation.ts', import.meta.url), 'utf8');

  test('submitGeneration / confirmGraphCreation 不直接调用 generateCourse', () => {
    const submitBody = hookSource.slice(
      hookSource.indexOf('const submitGeneration = useCallback('),
      hookSource.indexOf('const retryGeneration = useCallback(')
    );
    expect(submitBody.length).toBeGreaterThan(0);
    expect(submitBody).not.toContain('generateCourse(');
    expect(submitBody).toContain('submitPlan(');
  });

  test('closeGenerator 只隐藏对话框，不发任何请求', () => {
    const closeBody = hookSource.slice(
      hookSource.indexOf('const closeGenerator = useCallback('),
      hookSource.indexOf('// 生成完成后进入课程')
    );
    expect(closeBody.length).toBeGreaterThan(0);
    expect(closeBody).not.toContain('generateCourse');
    expect(closeBody).not.toContain('estimateGeneration');
  });

  test('计划态确认是唯一的生成接线（另一处仅失败/取消重试）', () => {
    expect(hookSource.split('void generateCourse(').length - 1).toBe(2);
  });
});
