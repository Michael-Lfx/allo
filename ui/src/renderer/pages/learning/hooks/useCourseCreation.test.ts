/** useCourseCreation 恢复态终态收敛（c2）：页面挂载时 refreshGenerationStatus
 * 从快照重建的 running/queued 态必须被推到终态——done 按快照行 course_id
 * 拉课程详情交 completed，failed 带后端 detail，cancelled 中性收场，任务从
 * 快照消失按取消兜底，绝不永久挂起。
 *
 * 可测面（决策逻辑抽离，跟随 autoRefill / useBrowserInventory 的形态）：
 * - `restoredGenerationFromTask`：恢复态重建（状态映射；请求形状归
 *   coursePlan.buildRestoredRequest 单源，c1 已在 coursePlan.test.ts 钉矩阵）；
 * - `startRestoredGenerationConvergence`：收敛轮询循环，数据源（快照/课程
 *   详情）全部注入——假数据源即测试替身，零 HTTP、零 DOM；
 * - hook 接线经 SSR 探针取得（renderToStaticMarkup：零 DOM、零新依赖）。
 * 隔离口径同 useCourseLearning.test：模型选择只透传 mock（仅本文件用例运行
 * 期间改答，其余时刻委托真实实现）；learningApi 是 HTTP 边界，逐用例打补丁
 * 并还原。 */
import * as bunTest from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { AppMessage } from '@/renderer/components/notifications';
import type { CourseDetail, LearningGenerationTaskView } from '../types';
import { learningApi } from '../api';
import type { Translate } from '../utils';

const { afterEach, beforeEach, describe, expect, test } = bunTest;
const mock = (bunTest as unknown as { mock: { module: (specifier: string, factory: () => unknown) => void } }).mock;

// 先按值取下真实导出再 mock：bun 对已加载模块原地打补丁，持命名空间引用会自指。
const realSelector = { ...(await import('../components/LearningModelSelector')) };
const realUseLearningAutogenModel = realSelector.useLearningAutogenModel;

let armed = false;

mock.module('../components/LearningModelSelector', () => ({
  ...realSelector,
  useLearningAutogenModel: () =>
    armed ? { choice: null, setChoice: async () => undefined } : realUseLearningAutogenModel(),
}));

const {
  RESTORED_GENERATION_POLL_MS,
  restoredGenerationFromTask,
  retryableGenerationRequest,
  startRestoredGenerationConvergence,
  useCourseCreation,
} = await import('./useCourseCreation');
type RestoredGenerationConvergence = import('./useCourseCreation').RestoredGenerationConvergence;
type CourseGenerationState = import('./useCourseCreation').CourseGenerationState;

interface FakeTimerApi {
  useFakeTimers: () => void;
  useRealTimers: () => void;
  clearAllTimers: () => void;
  advanceTimersByTime: (milliseconds: number) => void;
}

const timers = (bunTest as unknown as { jest: FakeTimerApi }).jest;

/** 冲刷微任务队列：轮询体内的 await 链（快照 → 课程详情 → 写回）逐跳推进。 */
const flushPromises = async () => {
  for (let hop = 0; hop < 20; hop += 1) await Promise.resolve();
};

/** 推进一个轮询周期并等轮询体跑完。 */
const tick = async () => {
  timers.advanceTimersByTime(RESTORED_GENERATION_POLL_MS);
  await flushPromises();
};

async function withFakeTimers(body: () => Promise<void>) {
  timers.useFakeTimers();
  try {
    await body();
  } finally {
    timers.clearAllTimers();
    timers.useRealTimers();
  }
}

const TASK_KEY = 'structure:course-1:-';

const task = (overrides: Partial<LearningGenerationTaskView> = {}): LearningGenerationTaskView => ({
  key: TASK_KEY,
  kind: 'structure',
  course_id: 'course-1',
  node_id: null,
  topic: '系统掌握 Rust 所有权',
  state: 'running',
  step: null,
  detail: null,
  elapsed_secs: 3,
  queued_at_ms: null,
  queue_position: null,
  ...overrides,
});

const courseDetail = (id: string): CourseDetail => ({
  course: {
    id,
    title: `课程 ${id}`,
    description: '',
    domain: '',
    source_kb_id: null,
    source_kb_ids: [],
    version: 1,
    enrolled: false,
    total_lessons: 0,
    completed_lessons: 0,
    enabled: true,
    settled_or_completed: 0,
    due_today: 0,
    total_xp: null,
    updated_at: 0,
    tags: [],
    course_kind: 'learning_graph',
    learning_goal: null,
    sampling: null,
    source_plan: null,
  },
  enrollment_id: null,
  modules: [],
  next_lesson_id: null,
  due_review_count: 0,
  overloaded_lessons: 0,
  graph: null,
});

/** 任务翻 failed 的收敛错误文案走本地化（测试替身 = 与 locale JSON 同义的
 * 人面句）；后端中文 detail 不进 error 面。 */
const FAILED_COPY_STUB = '后台任务已失败。';

/** 假数据源驱动一轮收敛：快照按轮询序逐帧给出（末帧常驻；Error 帧 = 快照
 * 不可达），课程详情按 id 应答并记账。 */
function startConvergence(options: {
  frames: Array<LearningGenerationTaskView[] | Error>;
  fetchCourse?: (courseId: string) => Promise<CourseDetail>;
  onRunning?: () => void;
}) {
  const converged: RestoredGenerationConvergence[] = [];
  const courseRequests: string[] = [];
  let runningCalls = 0;
  let polls = 0;
  const stop = startRestoredGenerationConvergence({
    taskKey: TASK_KEY,
    fetchTasks: async () => {
      const frame = options.frames[Math.min(polls, options.frames.length - 1)];
      polls += 1;
      if (frame instanceof Error) throw frame;
      return frame;
    },
    fetchCourse: async (courseId) => {
      courseRequests.push(courseId);
      return options.fetchCourse ? options.fetchCourse(courseId) : courseDetail(courseId);
    },
    describeError: (error) => (error instanceof Error ? error.message : 'unknown'),
    describeFailed: () => FAILED_COPY_STUB,
    onConverge: (next) => converged.push(next),
    onRunning: options.onRunning ?? (() => {
      runningCalls += 1;
    }),
  });
  return { stop, converged, courseRequests, runningCalls: () => runningCalls, polls: () => polls };
}

const t = ((key: string) => key) as unknown as Translate;

const realGenerationTasks = learningApi.generationTasks;
const realGetCourse = learningApi.getCourse;
const realCancelGeneration = learningApi.cancelGeneration;

beforeEach(() => {
  armed = true;
});

afterEach(() => {
  armed = false;
  learningApi.generationTasks = realGenerationTasks;
  learningApi.getCourse = realGetCourse;
  learningApi.cancelGeneration = realCancelGeneration;
  AppMessage.clear();
});

describe('restoredGenerationFromTask：从快照行重建对话框生成态', () => {
  test('running 行 → running 态，主题即描述、结构类任务标 learning_graph', () => {
    expect(restoredGenerationFromTask(task())).toEqual({
      status: 'running',
      request: { course_kind: 'learning_graph', description: '系统掌握 Rust 所有权' },
      // c3：快照机器面留存，悬浮 Pill 副行显示文案由词典按 kind 组装
      restoredTask: { kind: 'structure', topic: '系统掌握 Rust 所有权' },
      result: null,
      error: null,
    });
  });

  test('queued 行（资源闸排队中）→ queued 态', () => {
    expect(restoredGenerationFromTask(task({ state: 'queued', queue_position: 2 }))?.status).toBe(
      'queued'
    );
  });

  test('请求形状由 buildRestoredRequest 单源给出（c1）：v2 类任务不带 course_kind', () => {
    const restored = restoredGenerationFromTask(task({ kind: 'outline' }));
    if (!restored) throw new Error('expected restored state');
    expect(restored.request).toEqual({ description: '系统掌握 Rust 所有权' });
    expect('course_kind' in restored.request).toBe(false);
  });

  test('无主题的行不可恢复', () => {
    expect(restoredGenerationFromTask(task({ topic: '' }))).toBeNull();
  });
});

/** 恢复态终态：收敛写回保留 restoredTask，request 仍是按快照 topic 合成的
 * 那一份（description = 后端 topic 标签）。 */
const restoredTerminal = (status: 'failed' | 'cancelled'): CourseGenerationState => {
  const restored = restoredGenerationFromTask(task({ topic: '知识结构生成' }));
  if (!restored) throw new Error('expected restored state');
  return { ...restored, status, error: status === 'failed' ? '后台任务已失败' : null };
};

describe('retryableGenerationRequest：重试守卫', () => {
  test('恢复态 failed / cancelled 不可重发——合成请求会以后端 topic 标签为名新建课程', () => {
    expect(retryableGenerationRequest(restoredTerminal('failed'))).toBeNull();
    expect(retryableGenerationRequest(restoredTerminal('cancelled'))).toBeNull();
  });

  test('in-session failed / cancelled：按原请求重试（行为不变）', () => {
    const request = {
      course_kind: 'learning_graph' as const,
      description: '通盘认识股票市场基础',
    };
    const inSession = (status: 'failed' | 'cancelled'): CourseGenerationState => ({
      status,
      request,
      restoredTask: null,
      result: null,
      error: status === 'failed' ? '网络错误' : null,
    });
    expect(retryableGenerationRequest(inSession('failed'))).toEqual(request);
    expect(retryableGenerationRequest(inSession('cancelled'))).toEqual(request);
  });

  test('非终态（queued/running/completed）与无生成态不可重试', () => {
    expect(retryableGenerationRequest(null)).toBeNull();
    for (const status of ['queued', 'running', 'completed'] as const) {
      const state: CourseGenerationState = {
        status,
        request: { description: 'x' },
        restoredTask: null,
        result: null,
        error: null,
      };
      expect(retryableGenerationRequest(state)).toBeNull();
    }
  });
});

describe('startRestoredGenerationConvergence：恢复态终态收敛', () => {
  test('恢复 running → 快照翻 done → completed，按快照行 course_id 拉课程详情', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [[task()], [task({ state: 'done', detail: '知识结构已生成' })]],
      });

      await tick();
      expect(run.converged).toEqual([]);

      await tick();
      expect(run.courseRequests).toEqual(['course-1']);
      expect(run.converged).toEqual([
        { status: 'completed', result: courseDetail('course-1'), error: null },
      ]);
    });
  });

  test('快照翻 failed → failed，错误是本地化文案（后端中文 detail 不进 error 面）', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [[task()], [task({ state: 'failed', detail: 'Provider 连续失败，已停止' })]],
      });

      await tick();
      await tick();
      expect(run.courseRequests).toEqual([]);
      expect(run.converged).toEqual([{ status: 'failed', result: null, error: FAILED_COPY_STUB }]);

      // detail 缺席同样收敛为 failed，且 error 非空（宁缺不泄漏，不挂空）。
      const noDetail = startConvergence({
        frames: [[task()], [task({ state: 'failed', detail: null })]],
      });
      await tick();
      await tick();
      expect(noDetail.converged).toEqual([
        { status: 'failed', result: null, error: FAILED_COPY_STUB },
      ]);
    });
  });

  test('快照翻 cancelled → cancelled（中性终态，无错误文案）', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [[task()], [task({ state: 'cancelled', detail: '已取消' })]],
      });

      await tick();
      await tick();
      expect(run.converged).toEqual([{ status: 'cancelled', result: null, error: null }]);
    });
  });

  test('任务从快照消失（注册表被清）→ 按取消兜底，绝不永久挂起', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [[task()], []],
      });

      await tick();
      await tick();
      expect(run.converged).toEqual([{ status: 'cancelled', result: null, error: null }]);
    });
  });

  test('快照不可达（后端重启 / 断网）不臆造终态，下一轮继续取', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [new Error('network down'), new Error('network down'), [task({ state: 'done' })]],
      });

      await tick();
      await tick();
      expect(run.polls()).toBe(2);
      expect(run.converged).toEqual([]);

      await tick();
      expect(run.converged).toEqual([
        { status: 'completed', result: courseDetail('course-1'), error: null },
      ]);
    });
  });

  test('done 但课程详情拉取失败 → failed 带原因，不挂起', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [[task({ state: 'done' })]],
        fetchCourse: async () => {
          throw new Error('课程不存在');
        },
      });

      await tick();
      expect(run.converged).toEqual([{ status: 'failed', result: null, error: '课程不存在' }]);

      await tick();
      expect(run.polls()).toBe(1);
    });
  });

  test('running / queued 期间持续轮询，终态写回后停表', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [
          [task({ state: 'queued', queue_position: 1 })],
          [task()],
          [task()],
          [task({ state: 'done' })],
        ],
      });

      // 恢复动作刚读过一次快照：循环首个周期在间隔之后。
      expect(run.polls()).toBe(0);
      await tick();
      await tick();
      await tick();
      expect(run.polls()).toBe(3);
      expect(run.converged).toEqual([]);

      await tick();
      expect(run.polls()).toBe(4);
      expect(run.converged).toHaveLength(1);

      await tick();
      await tick();
      expect(run.polls()).toBe(4);
      expect(run.converged).toHaveLength(1);
    });
  });

  test('恢复 queued → 快照开跑 → 报 running 推进（非终态，轮询继续）', async () => {
    await withFakeTimers(async () => {
      const run = startConvergence({
        frames: [
          [task({ state: 'queued', queue_position: 1 })],
          [task()],
          [task({ state: 'done' })],
        ],
      });

      // 仍在排队：不推进、不收敛。
      await tick();
      expect(run.runningCalls()).toBe(0);
      expect(run.converged).toEqual([]);

      // 开跑：报一次 running 推进，界面不再停「排队中」；轮询继续等终态。
      await tick();
      expect(run.runningCalls()).toBe(1);
      expect(run.converged).toEqual([]);

      // 之后照常收敛到终态并停表。
      await tick();
      expect(run.polls()).toBe(3);
      expect(run.runningCalls()).toBe(1);
      expect(run.converged).toEqual([
        { status: 'completed', result: courseDetail('course-1'), error: null },
      ]);
    });
  });

  test('stop 之后不再轮询，在途读数也不写回（in-session 接管时防双写）', async () => {
    await withFakeTimers(async () => {
      const converged: RestoredGenerationConvergence[] = [];
      let polls = 0;
      let release: (tasks: LearningGenerationTaskView[]) => void = () => undefined;
      const stop = startRestoredGenerationConvergence({
        taskKey: TASK_KEY,
        fetchTasks: () => {
          polls += 1;
          return new Promise<LearningGenerationTaskView[]>((resolve) => {
            release = resolve;
          });
        },
        fetchCourse: async (courseId) => courseDetail(courseId),
        describeError: () => 'unused',
        describeFailed: () => 'unused',
        onConverge: (next) => {
          converged.push(next);
        },
      });

      await tick();
      expect(polls).toBe(1);

      // 在途期间接管（如 in-session 生成启动 / 卸载）：读数返回也不得写回。
      stop();
      release([task({ state: 'failed', detail: '迟到读数' })]);
      await flushPromises();
      expect(converged).toEqual([]);

      await tick();
      await tick();
      expect(polls).toBe(1);
    });
  });
});

/** SSR 探针挂载 hook（单次渲染，不重渲染）：动作面直接调用，状态迁移经
 * HTTP 边界补丁的读数次数观察。 */
function mountCourseCreation(translate: Translate = t) {
  let hook: ReturnType<typeof useCourseCreation> | undefined;
  function Probe(): null {
    hook = useCourseCreation({
      navigate: () => undefined,
      t: translate,
      setBusyId: () => undefined,
    });
    return null;
  }
  renderToStaticMarkup(React.createElement(Probe));
  if (!hook) throw new Error('probe did not render');
  return hook;
}

describe('useCourseCreation 接线', () => {
  test('refreshGenerationStatus 恢复 running 后启动收敛轮询：翻 done 按 course_id 拉详情并停表', async () => {
    await withFakeTimers(async () => {
      let snapshots = 0;
      const courseRequests: string[] = [];
      learningApi.generationTasks = async () => {
        snapshots += 1;
        return snapshots === 1 ? [task()] : [task({ state: 'done' })];
      };
      learningApi.getCourse = async (courseId) => {
        courseRequests.push(courseId);
        return courseDetail(courseId);
      };

      const hook = mountCourseCreation();
      await hook.refreshGenerationStatus();
      expect(snapshots).toBe(1);
      expect(courseRequests).toEqual([]);

      await tick();
      expect(snapshots).toBe(2);
      expect(courseRequests).toEqual(['course-1']);

      await tick();
      await tick();
      expect(snapshots).toBe(2);
    });
  });

  test('恢复 running → cancelGeneration → 快照翻 cancelled → 收敛且停表', async () => {
    await withFakeTimers(async () => {
      let snapshots = 0;
      let cancels = 0;
      learningApi.generationTasks = async () => {
        snapshots += 1;
        return cancels === 0 ? [task()] : [task({ state: 'cancelled', detail: '已取消' })];
      };
      learningApi.cancelGeneration = async () => {
        cancels += 1;
        return { cancelled: true };
      };

      const hook = mountCourseCreation();
      await hook.refreshGenerationStatus();
      expect(snapshots).toBe(1);

      await hook.cancelGeneration();
      expect(cancels).toBe(1);

      await tick();
      expect(snapshots).toBe(2);

      await tick();
      await tick();
      expect(snapshots).toBe(2);
    });
  });

  test('恢复 running → 快照翻 failed → 以本地化键出错误文案（后端 detail 不直达 UI）', async () => {
    await withFakeTimers(async () => {
      const tCalls: string[] = [];
      const recordingT = ((key: string) => {
        tCalls.push(key);
        return key;
      }) as unknown as Translate;
      let snapshots = 0;
      learningApi.generationTasks = async () => {
        snapshots += 1;
        return snapshots === 1 ? [task()] : [task({ state: 'failed', detail: '后端中文诊断句' })];
      };

      const hook = mountCourseCreation(recordingT);
      await hook.refreshGenerationStatus();
      await tick();
      await tick();
      expect(tCalls).toContain('learning.restoredTaskFailed');

      // 终态收敛后停表。
      await tick();
      expect(snapshots).toBe(2);
    });
  });

  test('无活跃建课任务时不启动轮询（生长任务不在此认领）', async () => {
    await withFakeTimers(async () => {
      let snapshots = 0;
      learningApi.generationTasks = async () => {
        snapshots += 1;
        return [task({ key: 'growth:course-1:-', kind: 'growth' })];
      };

      const hook = mountCourseCreation();
      await hook.refreshGenerationStatus();
      await tick();
      await tick();
      expect(snapshots).toBe(1);
    });
  });
});
