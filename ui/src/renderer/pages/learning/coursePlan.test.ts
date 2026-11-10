/** 建课请求构造（d5 简化后）的单测：三入口校验 + 请求装配。计划确认页已
 * 移除——提交即生成，状态机/读数/估算面随 CoursePlanCard 一并删除；这里只
 * 钉「校验失败只回 i18n 键」与各入口的请求形状，以及恢复态任务的请求形状
 * （c1：只有学习图结构类任务标 learning_graph）。 */
import { describe, expect, test } from 'bun:test';
import {
  buildPlanRequest,
  buildRestoredRequest,
  restoredTaskTopic,
  type CourseCreationForm,
} from './coursePlan';
import type { Translate } from './utils';

const form = (overrides: Partial<CourseCreationForm> = {}): CourseCreationForm => ({
  tab: 'base',
  goal: '掌握 Rust 异步编程',
  description: '想系统学会 Rust 异步',
  knowledgeBaseIds: ['kb-1', 'kb-2'],
  domain: '工程',
  providerId: 'provider-a',
  model: 'model-a',
  ...overrides,
});

describe('buildPlanRequest（三入口共用构造）', () => {
  test('知识库流：目标、多库、领域、模型逐字段进请求', () => {
    const result = buildPlanRequest(form(), 5);
    expect(result).toEqual({
      kind: 'ready',
      request: {
        knowledge_base_ids: ['kb-1', 'kb-2'],
        goal: '掌握 Rust 异步编程',
        domain: '工程',
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
        provider_id: 'provider-a',
        model: 'model-a',
      },
    });
  });

  test('学习图流：只有目标 + course_kind 明确', () => {
    const result = buildPlanRequest(form({ tab: 'graph' }), 5);
    expect(result).toEqual({
      kind: 'ready',
      request: {
        course_kind: 'learning_graph',
        description: '想系统学会 Rust 异步',
        provider_id: 'provider-a',
        model: 'model-a',
      },
    });
  });

  test('学习图流不再携带声明与风格字段（d5：UI 已移除，后端缺省接管）', () => {
    const result = buildPlanRequest(form({ tab: 'graph' }), 5);
    expect(result.kind).toBe('ready');
    if (result.kind !== 'ready') return;
    expect('proficiency_statement' in result.request).toBe(false);
    expect('learner_basis' in result.request).toBe(false);
    expect('teaching_style' in result.request).toBe(false);
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
    expect(buildPlanRequest(form({ tab: 'description', description: '' }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.describeRequired',
    });
    expect(
      buildPlanRequest(form({ providerId: undefined, model: undefined }), 5)
    ).toEqual({
      kind: 'invalid',
      messageKey: 'learning.form.modelPending',
    });
  });

  test('知识库上限校验由调用方口径传入', () => {
    expect(buildPlanRequest(form({ knowledgeBaseIds: ['a', 'b', 'c', 'd', 'e', 'f'] }), 5)).toEqual({
      kind: 'invalid',
      messageKey: 'learning.kbSelectionTooMany',
      messageVars: { count: 5 },
    });
  });
});

describe('buildRestoredRequest（恢复态任务 → 建课请求，c1）', () => {
  test('学习图结构类任务标 learning_graph（对话框走学习图最小呈现）', () => {
    for (const kind of ['structure', 'structure_mount', 'structure_repair']) {
      expect(buildRestoredRequest({ kind, topic: '知识结构生成' })).toEqual({
        course_kind: 'learning_graph',
        description: '知识结构生成',
      });
    }
  });

  test('v2 建课（outline）与课时任务不带 course_kind——呈现照旧走 v2', () => {
    for (const kind of ['outline', 'lesson']) {
      const request = buildRestoredRequest({ kind, topic: '掌握 Rust 异步编程' });
      expect(request).toEqual({ description: '掌握 Rust 异步编程' });
      expect('course_kind' in request).toBe(false);
    }
  });
});

/** 词典替身（独立真值 = 与 locale JSON 相同的字面值）：验证 restoredTaskTopic
 * 选了哪个 key、剥了什么标题，插值语义与 i18next {{var}} 同形。断言只关心
 * key 选择与标题剥取，措辞真值由 locale JSON + check:i18n 钉住。 */
const dict: Record<string, string> = {
  'learning.taskKindStructure': '知识结构生成',
  'learning.taskKindStructureMount': '结构挂载',
  'learning.taskKindStructureRepair': '结构修补',
  'learning.taskKindLesson': '课时内容 · {{title}}',
};
const tStub = ((key: string, vars?: Record<string, unknown>): string => {
  let out = dict[key] ?? `MISSING:${key}`;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      out = out.replaceAll(`{{${name}}}`, String(value));
    }
  }
  return out;
}) as unknown as Translate;

describe('restoredTaskTopic（恢复态任务 → 悬浮 Pill 显示文案，c3）', () => {
  test('结构类任务按 kind 出词典 label（后端 topic 不再直达 UI）', () => {
    expect(restoredTaskTopic({ kind: 'structure', topic: '知识结构生成' }, tStub)).toBe(
      '知识结构生成'
    );
    expect(restoredTaskTopic({ kind: 'structure_mount', topic: '结构挂载' }, tStub)).toBe(
      '结构挂载'
    );
    expect(restoredTaskTopic({ kind: 'structure_repair', topic: '结构修补' }, tStub)).toBe(
      '结构修补'
    );
  });

  test('课时任务剥「课时内容 · 」前缀取标题，词典模板带 title', () => {
    expect(restoredTaskTopic({ kind: 'lesson', topic: '课时内容 · 线性代数' }, tStub)).toBe(
      '课时内容 · 线性代数'
    );
  });

  test('课时任务前缀对不上（旧快照/措辞变更）：原样展示，不丢信息不双拼前缀', () => {
    expect(restoredTaskTopic({ kind: 'lesson', topic: '线性代数' }, tStub)).toBe('线性代数');
  });

  test('outline 与未知 kind：topic 原样（outline 本就是用户自述文本，语言中立）', () => {
    expect(restoredTaskTopic({ kind: 'outline', topic: '掌握 Rust 异步编程' }, tStub)).toBe(
      '掌握 Rust 异步编程'
    );
    expect(restoredTaskTopic({ kind: 'future_kind', topic: '任意' }, tStub)).toBe('任意');
  });
});
