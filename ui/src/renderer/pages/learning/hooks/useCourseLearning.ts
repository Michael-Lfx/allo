import { useCallback, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useLearningAutogenModel } from '../components/LearningModelSelector';
import { learningApi } from '../api';
import type {
  Activity,
  AttemptResult,
  DiagnosticPlan,
  GenerateLessonRequest,
  Lesson,
  LessonStatus,
  SubmitAttemptRequest,
} from '../types';
import { errorMessage, type Translate } from '../utils';

export interface UseCourseLearningOptions {
  id?: string;
  load: () => Promise<void>;
  t: Translate;
  diagnosticLimit: number | undefined;
  setBusyId: (id: string | null) => void;
}

/** 课程学习域：诊断测试、课时进度、活动作答（打开课程即自动加入） */
export function useCourseLearning({
  id,
  load,
  t,
  diagnosticLimit,
  setBusyId,
}: UseCourseLearningOptions) {
  const [attemptResults, setAttemptResults] = useState<Record<string, AttemptResult>>({});
  const [diagnosticPlan, setDiagnosticPlan] = useState<DiagnosticPlan | null>(null);
  const [diagnosticIndex, setDiagnosticIndex] = useState(0);
  const [diagnosticResult, setDiagnosticResult] = useState<AttemptResult>();
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

  // 预生成下一课时：尽力而为，失败不影响学习流程（用户仍可手动点击生成）
  const prefetching = useRef(new Set<string>());
  const prefetchLesson = useCallback(
    async (id: string) => {
      if (prefetching.current.has(id)) return;
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
    [generateRequest, load]
  );

  // 按需生成单个课时：生成当前课时并刷新；随后尽力预生成紧随其后的未生成课时
  const generateLesson = useCallback(
    async (lesson: Lesson, allLessons?: Lesson[]) => {
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
    [generateRequest, load, prefetchLesson, setBusyId, t]
  );

  const startDiagnostic = useCallback(async () => {
    if (!id) return;
    setBusyId('diagnostic');
    try {
      const plan = await learningApi.getDiagnostic(id, diagnosticLimit);
      if (plan.items.length === 0) {
        Message.warning(t('learning.noDiagnosticQuestions'));
        return;
      }
      setDiagnosticIndex(0);
      setDiagnosticResult(undefined);
      setDiagnosticPlan(plan);
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusyId(null);
    }
  }, [id, t, diagnosticLimit, setBusyId]);

  const submitDiagnostic = useCallback(
    async (activity: Activity, response: unknown) => {
      setBusyId(activity.id);
      try {
        const result = await learningApi.submitAttempt(activity.id, attemptRequest(response));
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        setDiagnosticResult(result);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [attemptRequest, t, setBusyId]
  );

  const advanceDiagnostic = useCallback(() => {
    if (!diagnosticPlan) return;
    if (diagnosticIndex < diagnosticPlan.items.length - 1) {
      setDiagnosticIndex((current) => current + 1);
      setDiagnosticResult(undefined);
      return;
    }
    setDiagnosticPlan(null);
    setDiagnosticResult(undefined);
    Message.success(t('learning.diagnosticComplete'));
    void load();
  }, [diagnosticIndex, diagnosticPlan, load, t]);

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
      setBusyId(activity.id);
      try {
        const result = await learningApi.submitAttempt(activity.id, attemptRequest(response));
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        await load();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [attemptRequest, load, t, setBusyId]
  );

  return {
    attemptResults,
    diagnosticPlan,
    diagnosticIndex,
    diagnosticResult,
    setDiagnosticPlan,
    setDiagnosticResult,
    startDiagnostic,
    submitDiagnostic,
    advanceDiagnostic,
    updateProgress,
    submitAttempt,
    generateLesson,
  };
}
