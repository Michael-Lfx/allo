import { useCallback, useState } from 'react';
import { Message } from '@arco-design/web-react';
import { learningApi } from '../api';
import type {
  Activity,
  AttemptResult,
  CourseDetail,
  DiagnosticPlan,
  Lesson,
  LessonStatus,
} from '../types';
import { errorMessage, type Translate } from '../utils';

export interface UseCourseLearningOptions {
  id?: string;
  load: () => Promise<void>;
  t: Translate;
  diagnosticLimit: number | undefined;
  setBusyId: (id: string | null) => void;
  setDetail: (detail: CourseDetail | null) => void;
}

/** 课程学习域：报名、诊断测试、课时进度、活动作答 */
export function useCourseLearning({
  id,
  load,
  t,
  diagnosticLimit,
  setBusyId,
  setDetail,
}: UseCourseLearningOptions) {
  const [attemptResults, setAttemptResults] = useState<Record<string, AttemptResult>>({});
  const [diagnosticPlan, setDiagnosticPlan] = useState<DiagnosticPlan | null>(null);
  const [diagnosticIndex, setDiagnosticIndex] = useState(0);
  const [diagnosticResult, setDiagnosticResult] = useState<AttemptResult>();

  const enroll = useCallback(async () => {
    if (!id) return;
    setBusyId(id);
    try {
      setDetail(await learningApi.enroll(id));
      await load();
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusyId(null);
    }
  }, [id, load, t, setBusyId]);

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
        const result = await learningApi.submitAttempt(activity.id, response);
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        setDiagnosticResult(result);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
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
        const result = await learningApi.submitAttempt(activity.id, response);
        setAttemptResults((current) => ({ ...current, [activity.id]: result }));
        await load();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  return {
    attemptResults,
    diagnosticPlan,
    diagnosticIndex,
    diagnosticResult,
    setDiagnosticPlan,
    setDiagnosticResult,
    enroll,
    startDiagnostic,
    submitDiagnostic,
    advanceDiagnostic,
    updateProgress,
    submitAttempt,
  };
}
