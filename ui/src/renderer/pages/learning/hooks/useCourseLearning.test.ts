/** useCourseLearning.submitAttempt 的模型门（m4）：只有 AI 判分题型
 * （reflection / open_question，镜像后端 ActivityKind::is_ai_graded）未选模型
 * 时被拦并提示选模型；规则判分的客观题零 AI 调用，未选模型也照常提交。
 *
 * hook 返回值经 SSR 探针取得（renderToStaticMarkup：零 DOM、零新依赖）。
 * 隔离口径——bun 的 mock.module 进程全局且跨文件常驻（test:ui 单进程跑全套）：
 * - 模型选择只透传 mock：仅本文件用例运行期间改答，其余时刻委托真实实现；
 * - learningApi 是 HTTP 边界：逐用例给 submitAttempt 打补丁并还原；
 * - 提示走真实通知仓，经其公开快照（NotificationHost 渲染所读同一份）观察。 */
import * as bunTest from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { AppMessage, notificationStore } from '@/renderer/components/notifications';
import type { ProviderId } from '@/common/types/ids';
import type { LearningModelChoice } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type { Activity, ActivityKind, SubmitAttemptRequest } from '../types';
import type { Translate } from '../utils';

const { afterEach, beforeEach, describe, expect, test } = bunTest;
const mock = (bunTest as unknown as { mock: { module: (specifier: string, factory: () => unknown) => void } }).mock;

// 先按值取下真实导出再 mock：bun 对已加载模块原地打补丁，持命名空间引用会自指。
const realSelector = { ...(await import('../components/LearningModelSelector')) };
const realUseLearningAutogenModel = realSelector.useLearningAutogenModel;

let armed = false;
let modelChoice: LearningModelChoice = null;

mock.module('../components/LearningModelSelector', () => ({
  ...realSelector,
  useLearningAutogenModel: () => (armed ? { choice: modelChoice } : realUseLearningAutogenModel()),
}));

const { useCourseLearning } = await import('./useCourseLearning');

const t = ((key: string) => key) as unknown as Translate;

const activity = (kind: ActivityKind): Activity => ({
  id: `act-${kind}`,
  kind,
  prompt: `prompt-${kind}`,
  options: kind === 'single_choice' ? ['A', 'B', 'C'] : [],
  matches: [],
  section_key: null,
  position: 0,
});

const submitted: Array<{ id: string; request: SubmitAttemptRequest }> = [];
const realSubmitAttempt = learningApi.submitAttempt;

beforeEach(() => {
  armed = true;
  modelChoice = null;
  submitted.length = 0;
  learningApi.submitAttempt = async (id, request) => {
    submitted.push({ id, request });
    return { id: `attempt-${id}`, score: 1, passed: true, feedback: '' };
  };
});

afterEach(() => {
  armed = false;
  learningApi.submitAttempt = realSubmitAttempt;
  AppMessage.clear();
});

function mountCourseLearning() {
  let loads = 0;
  let hook: ReturnType<typeof useCourseLearning> | undefined;
  function Probe(): null {
    hook = useCourseLearning({
      load: async () => {
        loads += 1;
      },
      t,
      setBusyId: () => undefined,
    });
    return null;
  }
  renderToStaticMarkup(React.createElement(Probe));
  if (!hook) throw new Error('probe did not render');
  return { submitAttempt: hook.submitAttempt, loads: () => loads };
}

/** 动作期间新弹出的提示（按 key 与动作前快照求差，不受他处残留影响） */
async function shownDuring(action: () => Promise<void>) {
  const before = new Set(notificationStore.getSnapshot().map((record) => record.key));
  await action();
  return notificationStore
    .getSnapshot()
    .filter((record) => !before.has(record.key))
    .map((record) => ({ level: record.level, content: record.content }));
}

describe('useCourseLearning.submitAttempt 模型门', () => {
  const objectiveCases: Array<{ kind: ActivityKind; response: unknown }> = [
    { kind: 'single_choice', response: 'B' },
    { kind: 'numeric', response: 42 },
  ];
  for (const { kind, response } of objectiveCases) {
    test(`规则判分题 ${kind} 未选模型也照常提交并刷新`, async () => {
      const { submitAttempt, loads } = mountCourseLearning();
      const shown = await shownDuring(() => submitAttempt(activity(kind), response));
      expect(shown).toEqual([]);
      expect(submitted).toEqual([
        { id: `act-${kind}`, request: { response, provider_id: undefined, model: undefined } },
      ]);
      expect(loads()).toBe(1);
    });
  }

  for (const kind of ['reflection', 'open_question'] as const) {
    test(`AI 判分题 ${kind} 未选模型被拦并提示选模型`, async () => {
      const { submitAttempt, loads } = mountCourseLearning();
      const shown = await shownDuring(() => submitAttempt(activity(kind), '我的作答'));
      expect(shown).toEqual([{ level: 'warning', content: 'learning.form.modelPending' }]);
      expect(submitted).toEqual([]);
      expect(loads()).toBe(0);
    });
  }

  test('reflection 选了模型正常提交并携带所选模型', async () => {
    modelChoice = { provider_id: 'provider-1' as ProviderId, model: 'model-1' };
    const { submitAttempt, loads } = mountCourseLearning();
    const shown = await shownDuring(() => submitAttempt(activity('reflection'), '我的作答'));
    expect(shown).toEqual([]);
    expect(submitted).toEqual([
      { id: 'act-reflection', request: { response: '我的作答', provider_id: 'provider-1', model: 'model-1' } },
    ]);
    expect(loads()).toBe(1);
  });
});
