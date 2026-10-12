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

  // 预生成下一课时：尽力而为，失败不影响学习流程（用户仍可手动点击生成）；
  // 模型未就绪时直接跳过，绝不用后端默认模型发起隐式生成。
  const prefetching = useRef(new Set<string>());
  const prefetchLesson = useCallback(
    async (id: string) => {
      if (!modelChoice || prefetching.current.has(id)) return;
      prefetching.current.add(id);
      try {
        await learningApi.generateLesson(id, generateRequest());
        await load();
      } catch {
        // 预生成失败可忽略
      } finally {
        prefetching.current.delete(id);
      }
    },
    [generateRequest, load, modelChoice]
  );

  // 按需生成单个课时：生成当前课时并刷新；随后尽力预生成紧随其后的未生成课时。
  // 模型未就绪时阻止无模型请求，不用后端默认模型代替用户选择。
  const generateLesson = useCallback(
    async (lesson: Lesson, allLessons?: Lesson[]) => {
      if (!modelChoice) {
        Message.warning(t('learning.form.modelPending'));
        return;
      }
      setBusyId(lesson.id);
      try {
        await learningApi.generateLesson(lesson.id, generateRequest());
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
    [generateRequest, load, modelChoice, prefetchLesson, setBusyId, t]
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
