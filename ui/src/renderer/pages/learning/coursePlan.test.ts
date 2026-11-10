/** 建课请求构造（d5 简化后）的单测：三入口校验 + 请求装配。计划确认页已
 * 移除——提交即生成，状态机/读数/估算面随 CoursePlanCard 一并删除；这里只
 * 钉「校验失败只回 i18n 键」与各入口的请求形状，以及恢复态任务的请求形状
 * （c1：只有学习图结构类任务标 learning_graph）。 */
import { describe, expect, test } from 'bun:test';
import { buildPlanRequest, buildRestoredRequest, type CourseCreationForm } from './coursePlan';

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
