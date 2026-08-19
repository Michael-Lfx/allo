import { useCallback, useEffect, useRef, useState } from 'react';
import { Message } from '@arco-design/web-react';
import { learningApi } from '../api';
import type { CourseJobView } from '../types';
import { errorMessage, type Translate } from '../utils';
import type { LearningModelChoice } from '../components/LearningModelSelector';

/** 非终态任务存在时的轮询间隔（毫秒） */
export const COURSE_JOB_POLL_MS = 3000;

export function isCourseJobTerminal(job: CourseJobView): boolean {
  return (
    job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled'
  );
}

export interface UseCourseJobsOptions {
  t: Translate;
  setBusyId: (id: string | null) => void;
  /** 有任务完成（课程已入库）时回调，用于刷新课程列表 */
  onJobCompleted: () => void;
}

/** 课程生成任务域：列表加载 + 非终态任务轮询 + 取消/继续/重试动作。
 * 动作 busyId 约定为 `job-${job_id}`，与页面 busyId 机制共用。 */
export function useCourseJobs({ t, setBusyId, onJobCompleted }: UseCourseJobsOptions) {
  const [jobs, setJobs] = useState<CourseJobView[]>([]);
  const [loading, setLoading] = useState(false);
  // 页面回调可能随渲染变化，用 ref 固定，避免轮询 effect 反复重启
  const onJobCompletedRef = useRef(onJobCompleted);
  onJobCompletedRef.current = onJobCompleted;
  const completedIdsRef = useRef<Set<string>>(new Set());
  // 存在非终态任务时为 true，驱动轮询的启停
  const [hasActive, setHasActive] = useState(false);

  const loadJobs = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const next = await learningApi.listCourseJobs();
      setJobs(next);
      const newlyCompleted = next.filter(
        (job) => job.status === 'completed' && !completedIdsRef.current.has(job.job_id)
      );
      if (newlyCompleted.length > 0) {
        newlyCompleted.forEach((job) => completedIdsRef.current.add(job.job_id));
        onJobCompletedRef.current();
      }
      // 每次加载都重算活跃态：重试/继续会让任务回到非终态，轮询随之恢复；
      // 全部终态后自动停止，而不是只在挂载时判断一次。
      setHasActive(next.some((job) => !isCourseJobTerminal(job)));
      return next;
    } catch {
      // 任务面板拉取失败不阻塞页面主体
      return null;
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  // 首次加载任务列表
  useEffect(() => {
    void loadJobs(false);
  }, [loadJobs]);

  // 存在非终态任务时每 3 秒轮询；全部终态后停止
  useEffect(() => {
    if (!hasActive) return;
    const timer = setInterval(() => void loadJobs(true), COURSE_JOB_POLL_MS);
    return () => clearInterval(timer);
  }, [hasActive, loadJobs]);

  const runAction = useCallback(
    async (jobId: string, action: () => Promise<unknown>, successKey: string) => {
      setBusyId(`job-${jobId}`);
      try {
        await action();
        Message.success(t(successKey));
        await loadJobs(true);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [loadJobs, setBusyId, t]
  );

  const cancelJob = useCallback(
    (jobId: string) =>
      runAction(jobId, () => learningApi.cancelCourseJob(jobId), 'learning.jobCancelRequested'),
    [runAction]
  );
  const resumeJob = useCallback(
    (jobId: string) =>
      runAction(jobId, () => learningApi.resumeCourseJob(jobId), 'learning.jobResumed'),
    [runAction]
  );
  const retryJob = useCallback(
    (jobId: string, choice: LearningModelChoice) =>
      runAction(
        jobId,
        () =>
          learningApi.retryCourseJob(jobId, {
            provider_id: choice?.provider_id,
            model: choice?.model,
          }),
        'learning.jobRetried'
      ),
    [runAction]
  );
  const deleteJob = useCallback(
    (jobId: string) => runAction(jobId, () => learningApi.deleteCourseJob(jobId), 'learning.jobDeleted'),
    [runAction]
  );

  return { jobs, loading, hasActive, cancelJob, resumeJob, retryJob, deleteJob };
}
