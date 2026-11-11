/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** quizSessionController 状态机单测（u3）。可测面是控制器公共 seam（hook 的
 * 状态源）：start → 逐题作答判分 → 下一题 → 全卷收集完整 → complete 交报告。
 * 假数据源即测试替身（useCourseCreation.test 同纪律）：api/notify/onFinished
 * 全部注入，零 HTTP、零 DOM。持答案红线（R4）与全卷收集门（R5）在本文件
 * 钉死：反馈只随判分出现、只随当前题、推进即弃、关闭全弃；response 原样
 * 回收、全题作答才允许 complete、会话内零 progress 调用。 */
import * as bunTest from 'bun:test';
import { BackendHttpError, BackendRequestError } from '@/common/adapter/httpBridge';
import type { QuizGradeFeedback, QuizRoundQuestionView, QuizRoundReport, QuizRoundView } from '../types';
import type { Translate } from '../utils';

const { describe, expect, test } = bunTest;

/** 记参翻译替身：断言词典键与插值参数双双如实（quizMissingAnswers 的 count）。 */
const t = ((key: string, params?: Record<string, unknown>) =>
  params === undefined ? key : `${key}:${JSON.stringify(params)}`) as unknown as Translate;

const question = (overrides: Partial<QuizRoundQuestionView> = {}): QuizRoundQuestionView => ({
  activity_id: 'act-1',
  kind: 'single_choice',
  prompt: '下列哪一个是无理数？',
  options: ['$\\sqrt{2}$', '$0.5$'],
  matches: [],
  node_id: 'node-1',
  node_name: '实数系',
  difficulty: 2,
  ...overrides,
});

const round = (questions: QuizRoundQuestionView[]): QuizRoundView => ({
  round_id: 'round-1',
  course_id: 'course-1',
  status: 'active',
  settled_count: 6,
  trigger_count: 6,
  budget: 15,
  questions,
});

const feedback = (activityId: string, correct: boolean): QuizGradeFeedback => ({
  activity_id: activityId,
  correct,
  answer: 'A',
  explanation: '因为 $\sqrt{2}$ 不能写成两整数之比。',
  evidence: null,
});

const report = (): QuizRoundReport => ({
  round_id: 'round-1',
  course_id: 'course-1',
  earned_xp: 30,
  covered: [],
});

interface QuizApiStub {
  quizStart: (courseId: string) => Promise<QuizRoundView>;
  quizGrade: (
    courseId: string,
    roundId: string,
    activityId: string,
    response: unknown
  ) => Promise<QuizGradeFeedback>;
  quizComplete: (
    courseId: string,
    roundId: string,
    answers: Record<string, unknown>
  ) => Promise<QuizRoundReport>;
  quizProgress: (courseId: string) => Promise<unknown>;
}

/** 假 quiz api：记账全部调用（R6 断言 progress 计数恒零）。overrides 逐方法
 * 覆盖（失败路径注入 reject）。 */
function makeApi(overrides: Partial<QuizApiStub> = {}) {
  const calls = {
    start: [] as string[],
    grade: [] as Array<{ courseId: string; roundId: string; activityId: string; response: unknown }>,
    complete: [] as Array<{ courseId: string; roundId: string; answers: Record<string, unknown> }>,
    progress: 0,
  };
  const roundView = round([
    question({ activity_id: 'act-1', node_name: '实数系' }),
    question({ activity_id: 'act-2', kind: 'multi_choice', node_name: '实数系' }),
    question({ activity_id: 'act-3', kind: 'true_false', node_name: '虚数系', difficulty: undefined }),
  ]);
  const api: QuizApiStub = {
    quizStart: async (courseId) => {
      calls.start.push(courseId);
      return roundView;
    },
    quizGrade: async (courseId, roundId, activityId, response) => {
      calls.grade.push({ courseId, roundId, activityId, response });
      return feedback(activityId, true);
    },
    quizComplete: async (courseId, roundId, answers) => {
      calls.complete.push({ courseId, roundId, answers });
      return report();
    },
    quizProgress: async () => {
      calls.progress += 1;
      return { settled_count: 6, trigger_count: 6, due: true };
    },
    ...overrides,
  };
  return { api, calls };
}

const { createQuizSessionController, initialQuizSessionState, missingAnswerCount } = await import(
  './useQuizSession'
);

interface Harness {
  controller: ReturnType<typeof createQuizSessionController>;
  api: QuizApiStub;
  calls: ReturnType<typeof makeApi>['calls'];
  reports: QuizRoundReport[];
  toasts: string[];
}

function makeHarness(apiOverrides: Partial<QuizApiStub> = {}): Harness {
  const { api, calls } = makeApi(apiOverrides);
  const reports: QuizRoundReport[] = [];
  const toasts: string[] = [];
  const controller = createQuizSessionController({
    courseId: 'course-1',
    t,
    onFinished: (finished) => reports.push(finished),
    onChange: () => undefined,
    api,
    notify: (message) => toasts.push(message),
  });
  return { controller, api, calls, reports, toasts };
}

describe('quizSessionController 全旅程（u3 R1/R5）', () => {
  test('start → 逐题作答判分 → 下一题 → 全卷收集完整 → complete 交报告（漏题不可能）', async () => {
    const { controller, calls, reports, toasts } = makeHarness();

    // 初始态：loading，无回合无反馈无收集
    expect(controller.getState()).toEqual(initialQuizSessionState);

    expect(await controller.start()).toBe(true);
    let state = controller.getState();
    expect(state.phase).toBe('answering');
    expect(state.round?.round_id).toBe('round-1');
    expect(state.index).toBe(0);
    expect(state.answers).toEqual({});
    expect(state.feedback).toBeNull();
    expect(calls.start).toEqual(['course-1']);

    // 题1：single_choice，response 字符串原样回收
    const response1 = '$\\sqrt{2}$';
    expect(await controller.grade(response1)).toBe(true);
    state = controller.getState();
    expect(state.phase).toBe('graded');
    expect(state.feedback?.activity_id).toBe('act-1');
    expect(state.feedback?.correct).toBe(true);
    expect(state.answers['act-1']).toBe(response1);

    expect(controller.next()).toBe(true);
    state = controller.getState();
    expect(state.phase).toBe('answering');
    expect(state.index).toBe(1);
    // R4：反馈随推进丢弃，收集保留
    expect(state.feedback).toBeNull();
    expect('act-1' in state.answers).toBe(true);

    // 题2：multi_choice，response 数组原样回收
    const response2 = ['$\\sqrt{2}$', '$0.5$'];
    expect(await controller.grade(response2)).toBe(true);
    state = controller.getState();
    expect(state.feedback?.activity_id).toBe('act-2');
    expect(state.answers['act-2']).toBe(response2);

    expect(controller.next()).toBe(true);
    expect(controller.getState().phase).toBe('answering');

    // 题3：true_false，response 布尔原样回收；最后一题 next → ready
    expect(await controller.grade(false)).toBe(true);
    expect(controller.getState().feedback?.activity_id).toBe('act-3');
    expect(controller.next()).toBe(true);
    state = controller.getState();
    expect(state.phase).toBe('ready');

    // R5：全卷收集完整——三题 response 与判分时引用一致，零漏题
    expect(Object.keys(state.answers).sort()).toEqual(['act-1', 'act-2', 'act-3']);

    expect(await controller.complete()).toBe(true);
    expect(calls.complete).toHaveLength(1);
    expect(calls.complete[0]?.courseId).toBe('course-1');
    expect(calls.complete[0]?.roundId).toBe('round-1');
    expect(calls.complete[0]?.answers['act-1']).toBe(response1);
    expect(calls.complete[0]?.answers['act-2']).toBe(response2);
    expect(calls.complete[0]?.answers['act-3']).toBe(false);
    // 会话结束回调携带全卷报告
    expect(reports).toHaveLength(1);
    expect(reports[0]?.earned_xp).toBe(30);
    expect(controller.getState().phase).toBe('completed');
    // R6：会话内零 progress 调用；全旅程零打扰 toast
    expect(calls.progress).toBe(0);
    expect(toasts).toEqual([]);
  });

  test('判分请求面如实：grade 以 (courseId, roundId, 当前题 activity_id, response 原样) 调用', async () => {
    const { controller, calls } = makeHarness();
    await controller.start();
    const response = { value: 'A' };
    await controller.grade(response);
    expect(calls.grade).toHaveLength(1);
    expect(calls.grade[0]).toEqual({
      courseId: 'course-1',
      roundId: 'round-1',
      activityId: 'act-1',
      response,
    });
  });
});

describe('quizSessionController 持答案红线与动作门（u3 R4）', () => {
  test('反馈只在判分后存在：作答相/初始态 feedback 恒空，判分失败不落反馈', async () => {
    const { controller } = makeHarness({
      quizGrade: async () => {
        throw new BackendHttpError({ method: 'POST', path: '/quiz/grade', status: 500, body: {} });
      },
    });
    await controller.start();
    expect(controller.getState().feedback).toBeNull();
    expect(await controller.grade('A')).toBe(false);
    const state = controller.getState();
    expect(state.phase).toBe('answering');
    expect(state.feedback).toBeNull();
    expect(state.busy).toBe(false);
    expect(state.answers).toEqual({});
  });

  test('反馈只随当前题：graded 相拒绝二次判分（状态引用不变），推进后旧反馈不残留', async () => {
    const { controller } = makeHarness();
    await controller.start();
    await controller.grade('A');
    const gradedState = controller.getState();
    expect(gradedState.feedback?.activity_id).toBe('act-1');
    // 反馈期再提交：拒绝且状态零变化（同引用 = 零迁移）
    expect(await controller.grade('B')).toBe(false);
    expect(controller.getState()).toBe(gradedState);
    // 推进后：反馈丢弃、收集保留
    expect(controller.next()).toBe(true);
    const next = controller.getState();
    expect(next.feedback).toBeNull();
    expect(next.phase).toBe('answering');
    expect('act-1' in next.answers).toBe(true);
  });

  test('作答相拒绝推进；busy 在途拒绝重复判分（单飞）', async () => {
    let releaseGrade: ((value: QuizGradeFeedback) => void) | undefined;
    const { controller } = makeHarness({
      quizGrade: async (_courseId, _roundId, activityId) =>
        new Promise<QuizGradeFeedback>((resolve) => {
          releaseGrade = resolve;
          void activityId;
        }),
    });
    await controller.start();
    void expect(controller.next()).toBe(false);
    const pending = controller.grade('A');
    // 在途：busy 置起，第二次提交被拒
    expect(controller.getState().busy).toBe(true);
    expect(await controller.grade('B')).toBe(false);
    releaseGrade?.(feedback('act-1', true));
    expect(await pending).toBe(true);
    expect(controller.getState().phase).toBe('graded');
  });

  test('R6 关闭即弃回合：反馈/收集/回合全清零，深等于初始态', async () => {
    const { controller } = makeHarness();
    await controller.start();
    await controller.grade('A');
    controller.close();
    expect(controller.getState()).toEqual(initialQuizSessionState);
    // 关闭后机器回到 loading 相，可重新 start（与「下次 start supersede」语义一致）
    expect(await controller.start()).toBe(true);
    expect(controller.getState().phase).toBe('answering');
  });

  test('complete 只在 ready 相可进：作答/判分相拒绝且零 API 调用', async () => {
    const { controller, calls } = makeHarness();
    await controller.start();
    void expect(await controller.complete()).toBe(false);
    await controller.grade('A');
    void expect(await controller.complete()).toBe(false);
    expect(calls.complete).toHaveLength(0);
  });

  test('R5 前置校验纯面：missingAnswerCount 全卷作答为 0，缺题如实计数', async () => {
    const roundView = round([
      question({ activity_id: 'act-1' }),
      question({ activity_id: 'act-2' }),
      question({ activity_id: 'act-3' }),
    ]);
    expect(missingAnswerCount(roundView, { 'act-1': 'a', 'act-2': 'b', 'act-3': true })).toBe(0);
    expect(missingAnswerCount(roundView, { 'act-1': 'a' })).toBe(2);
  });
});

describe('quizSessionController 失败路径（u3 Acceptance：409/网络如实落错误态）', () => {
  test('start 409（due 门冲突）→ error 相 + errorMessage 单源 conflictBusy 文案，回合零残留', async () => {
    const { controller, calls } = makeHarness({
      quizStart: async () => {
        throw new BackendHttpError({
          method: 'POST',
          path: '/api/learning/courses/course-1/quiz/start',
          status: 409,
          body: {},
        });
      },
    });
    expect(await controller.start()).toBe(false);
    const state = controller.getState();
    expect(state.phase).toBe('error');
    expect(state.error).toBe('learning.conflictBusy');
    expect(state.round).toBeNull();
    expect(state.feedback).toBeNull();
    expect(state.answers).toEqual({});
    expect(state.busy).toBe(false);
    expect(calls.grade).toHaveLength(0);
  });

  test('start 网络故障（传输面）→ error 相 + networkUnstable 文案', async () => {
    const { controller } = makeHarness({
      quizStart: async () => {
        throw new BackendRequestError('network', 'socket hang up');
      },
    });
    expect(await controller.start()).toBe(false);
    expect(controller.getState().phase).toBe('error');
    expect(controller.getState().error).toBe('learning.networkUnstable');
  });

  test('error 相重试同入口：第二次 start 成功回到 answering', async () => {
    let attempts = 0;
    const { controller } = makeHarness({
      quizStart: async () => {
        attempts += 1;
        if (attempts === 1) throw new BackendRequestError('timeout', 'timed out');
        return round([question()]);
      },
    });
    expect(await controller.start()).toBe(false);
    expect(controller.getState().phase).toBe('error');
    expect(await controller.start()).toBe(true);
    expect(controller.getState().phase).toBe('answering');
    expect(controller.getState().error).toBeNull();
  });

  test('start 契约外空回合 → 如实落 error 相（不渲染空题面）', async () => {
    const { controller } = makeHarness({
      quizStart: async () => ({ ...round([]) }),
    });
    expect(await controller.start()).toBe(false);
    const state = controller.getState();
    expect(state.phase).toBe('error');
    expect(state.error).toBe('learning.quizLoadFailed');
  });

  test('判分失败 → toast 提示、停在作答相可重交；成功重交后反馈落位', async () => {
    let attempts = 0;
    const { controller, toasts } = makeHarness({
      quizGrade: async () => {
        attempts += 1;
        if (attempts === 1) throw new BackendRequestError('aborted', 'request aborted');
        return feedback('act-1', false);
      },
    });
    await controller.start();
    expect(await controller.grade('A')).toBe(false);
    expect(toasts).toEqual(['learning.networkUnstable']);
    expect(controller.getState().phase).toBe('answering');
    expect(controller.getState().busy).toBe(false);
    // 重交成功：判底如实呈现（correct:false 的错误态反馈两态之一）
    expect(await controller.grade('A')).toBe(true);
    expect(controller.getState().feedback?.correct).toBe(false);
    expect(toasts).toHaveLength(1);
  });

  test('complete 失败 → toast 提示、停在 ready 相可重结算', async () => {
    let attempts = 0;
    const { controller, reports, toasts } = makeHarness({
      quizComplete: async () => {
        attempts += 1;
        if (attempts === 1) {
          throw new BackendHttpError({ method: 'POST', path: '/quiz/complete', status: 500, body: {} });
        }
        return report();
      },
    });
    await controller.start();
    for (let step = 0; step < 3; step += 1) {
      await controller.grade(`resp-${step}`);
      controller.next();
    }
    expect(controller.getState().phase).toBe('ready');
    expect(await controller.complete()).toBe(false);
    expect(toasts).toEqual(['learning.serviceUnavailable']);
    expect(controller.getState().phase).toBe('ready');
    expect(reports).toHaveLength(0);
    // 重试结算成功交报告
    expect(await controller.complete()).toBe(true);
    expect(attempts).toBe(2);
    expect(reports).toHaveLength(1);
  });

  test('在途 complete 拒绝重复结算', async () => {
    let releaseComplete: ((value: QuizRoundReport) => void) | undefined;
    const { controller } = makeHarness({
      quizComplete: async () =>
        new Promise<QuizRoundReport>((resolve) => {
          releaseComplete = resolve;
        }),
    });
    await controller.start();
    for (let step = 0; step < 3; step += 1) {
      await controller.grade(`resp-${step}`);
      controller.next();
    }
    const pending = controller.complete();
    expect(controller.getState().busy).toBe(true);
    expect(await controller.complete()).toBe(false);
    releaseComplete?.(report());
    expect(await pending).toBe(true);
    expect(controller.getState().phase).toBe('completed');
  });
});
