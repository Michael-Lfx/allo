import { useCallback, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type {
  Activity,
  AttemptRecord,
  AttemptResult,
  GenerateLessonRequest,
  Lesson,
  LessonStatus,
  SubmitAttemptRequest,
} from '../types';
import { errorMessage, type Translate } from '../utils';

export interface UseCourseLearningOptions {
  load: () => Promise<void>;
  t: Translate;
  setBusyId: (id: string | null) => void;
}

/** 课程学习域：课时进度、活动作答与按需生成（打开课程即自动加入） */
export function useCourseLearning({ load, t, setBusyId }: UseCourseLearningOptions) {
  const [attemptResults, setAttemptResults] = useState<Record<string, AttemptRecord>>({});
  // 学习页统一模型偏好：携带用户选择的 AI 模型批改反思题；未选择时后端回落默认模型
  const { choice: modelChoice } = useLearningAutogenModel();

  const attemptRequest = useCallback(
    (response: unknown): SubmitAttemptRequest => ({
      response,
      provider_id: modelChoice?.provider_id,
      model: modelChoice?.model,
    }),
    [modelChoice]
  );

  const generateRequest = useCallback(
    (): GenerateLessonRequest => ({
      provider_id: modelChoice?.provider_id,
      model: modelChoice?.model,
    }),
    [modelChoice]
  );

  // e4 异步臂轮询：任务键 → 终态。生成已脱离请求生命周期（断开连接不中
  // 断），前端轮询 generation-tasks 快照等 done；failed/cancelled 以任务
  // detail（后端结构化原因）抛错。快照短暂缺行（登记与查询的竞速窗口）
  // 继续等；总时长封顶 35 分钟 = 后端最坏路径对齐（3 次重试 × 单次尝试
  // 660s 死人开关 LESSON_ATTEMPT_DEADLINE ≈ 33 分钟，留 2 分钟轮询余量）
  // ——20 分钟会在后端仍在重试时前端先报超时假阴性。
  const pollLessonTask = useCallback(
    async (taskKey: string) => {
      const deadline = Date.now() + 35 * 60_000;
      while (Date.now() < deadline) {
        const tasks = await learningApi.generationTasks();
        const task = tasks.find((item) => item.key === taskKey);
        if (task) {
          if (task.state === 'done') return;
          if (task.state === 'failed') {
            throw new Error(task.detail || t('learning.lessonGenTaskFailed'));
          }
          if (task.state === 'cancelled') {
            throw new Error(t('learning.lessonGenTaskCancelled'));
          }
        }
        await new Promise((resolve) => setTimeout(resolve, 2000));
      }
      throw new Error(t('learning.lessonGenPollTimeout'));
    },
    [t]
  );

  // 预生成下一课时：尽力而为，失败不影响学习流程（用户仍可手动点击生成）；
  // 模型未就绪时直接跳过，绝不用后端默认模型发起隐式生成。e4：任务后台化
  // ——提交即入队，轮询终态后再刷新（预生成脱离本页生命周期，断连不中断）。
  const prefetching = useRef(new Set<string>());
  const prefetchLesson = useCallback(
    async (id: string) => {
      if (!modelChoice || prefetching.current.has(id)) return;
      prefetching.current.add(id);
      try {
        const accepted = await learningApi.generateLesson(id, generateRequest());
        await pollLessonTask(accepted.task_key);
        await load();
      } catch {
        // 预生成失败可忽略
      } finally {
        prefetching.current.delete(id);
      }
    },
    [generateRequest, load, modelChoice, pollLessonTask]
  );

  // 按需生成单个课时：提交后台任务 → 轮询终态 → 刷新；随后尽力预生成紧随
  // 其后的未生成课时。模型未就绪时阻止无模型请求，不用后端默认模型代替用
  // 户选择。
  const generateLesson = useCallback(
    async (lesson: Lesson, allLessons?: Lesson[]) => {
      if (!modelChoice) {
        Message.warning(t('learning.form.modelPending'));
        return;
      }
      setBusyId(lesson.id);
      try {
        const accepted = await learningApi.generateLesson(lesson.id, generateRequest());
        await pollLessonTask(accepted.task_key);
        await load();
        if (allLessons) {
          const index = allLessons.findIndex((item) => item.id === lesson.id);
          const next = allLessons.slice(index + 1).find((item) => !item.generated);
          if (next) void prefetchLesson(next.id);
        }
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [generateRequest, load, modelChoice, pollLessonTask, prefetchLesson, setBusyId, t]
  );

  const updateProgress = useCallback(
    async (lesson: Lesson, status: LessonStatus) => {
      setBusyId(lesson.id);
      try {
        await learningApi.updateLessonProgress(lesson.id, status);
        await load();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  const submitAttempt = useCallback(
    async (activity: Activity, response: unknown) => {
      if (!modelChoice) {
        Message.warning(t('learning.form.modelPending'));
        return;
      }
      setBusyId(activity.id);
      try {
        const result = await learningApi.submitAttempt(activity.id, attemptRequest(response));
        setAttemptResults((current) => ({ ...current, [activity.id]: { ...result, response } }));
        await load();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [attemptRequest, load, modelChoice, t, setBusyId]
  );

  return {
    attemptResults,
    updateProgress,
    submitAttempt,
    generateLesson,
  };
}
