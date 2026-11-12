import { useCallback, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { learningApi } from '../api';
import { ORPHAN_COURSE_FILTER } from '../constants';
import type { DueReview, ReviewAnswerResult, ReviewRating } from '../types';
import { errorMessage, type Translate } from '../utils';

/** 课程/节点集复习会话的队列上限：专注域内全部已入队卡片（不限于到期项），
 * 因此比日刷缺省上限更大。 */
const REVIEW_SESSION_QUEUE_LIMIT = 100;

export interface UseReviewSessionOptions {
  load: () => Promise<void>;
  t: Translate;
  reviewCourseFilter: string[];
  reviewTagFilter: string[];
  reviewSessionLimit: number | undefined;
  setBusyId: (id: string | null) => void;
  setReviews: (reviews: DueReview[]) => void;
}

/** 复习域：到期队列、会话弹窗与答题/遗忘/评分/跳过动作 */
export function useReviewSession({
  load,
  t,
  reviewCourseFilter,
  reviewTagFilter,
  reviewSessionLimit,
  setBusyId,
  setReviews,
}: UseReviewSessionOptions) {
  const [sessionOpen, setSessionOpen] = useState(false);
  const [sessionQueue, setSessionQueue] = useState<DueReview[]>([]);

  const answerReview = useCallback(
    async (
      review: DueReview,
      response: unknown,
      elapsedMs?: number
    ): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.source, review.id, response, false, elapsedMs);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  const forgetReview = useCallback(
    async (review: DueReview, elapsedMs?: number): Promise<ReviewAnswerResult | undefined> => {
      setBusyId(review.id);
      try {
        return await learningApi.answerReview(review.source, review.id, null, true, elapsedMs);
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return undefined;
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  const rateReview = useCallback(
    async (review: DueReview, rating: ReviewRating): Promise<boolean> => {
      setBusyId(review.id);
      try {
        const result = await learningApi.rateReview(review.source, review.id, rating);
        // 到期门：本学习日已推进过且未到期的过期重复不再推进排期
        if (result.advanced) {
          Message.success(t('learning.reviewRecorded'));
        } else {
          Message.info(t('learning.reviewGateBlocked'));
        }
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  const skipReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        await learningApi.skipReview(review.source, review.id);
        Message.success(t('learning.reviewSkipped'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 归档当前卡片：暂停出现但保留数据，可随时恢复 */
  const archiveReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.archiveCustomQuestion(review.id);
        } else {
          await learningApi.archiveReview(review.id);
        }
        Message.success(t('learning.reviewArchived'));
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 删除当前卡片：课程题移出队列，自定义题彻底删除 */
  const removeReview = useCallback(
    async (review: DueReview): Promise<boolean> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.deleteCustomQuestion(review.id);
          Message.success(t('learning.questionDeleted'));
        } else {
          await learningApi.deleteReviewItem(review.id);
          Message.success(t('learning.reviewRemovedFromQueue'));
        }
        await load();
        return true;
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId]
  );

  /** 标记待编辑：记录意图与选填描述，不推进队列，不打断复习心流 */
  const markEditPending = useCallback(
    async (review: DueReview, note: string): Promise<void> => {
      setBusyId(review.id);
      try {
        if (review.source === 'custom') {
          await learningApi.markCustomEditPending(review.id, note);
        } else {
          await learningApi.markReviewEditPending(review.id, note);
        }
        // 本地同步会话队列中的卡片状态，无需重拉队列
        setSessionQueue((prev) =>
          prev.map((item) =>
            item.id === review.id
              ? { ...item, edit_pending: true, edit_note: note.trim() || null }
              : item
          )
        );
        await load();
        Message.success(t('learning.reviewMarkEditMarked'));
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setBusyId(null);
      }
    },
    [load, t, setBusyId, setSessionQueue]
  );

  /** 会话开启骨架（日刷/课程/节点集三入口共用）：置忙 → 经 fetcher 重拉
   * 到期队列（避免使用会话期间过期的快照）→ 空队列仅提示不开窗 → 入队
   * 开窗；拉取异常降级为错误 toast。返回是否真正开窗。 */
  const openSession = useCallback(
    async (fetcher: () => Promise<DueReview[]>, emptyMessageKey: string): Promise<boolean> => {
      setBusyId('review-session');
      try {
        const fresh = await fetcher();
        if (fresh.length === 0) {
          Message.info(t(emptyMessageKey));
          return false;
        }
        setSessionQueue(fresh);
        setSessionOpen(true);
        return true;
      } catch (sessionError) {
        Message.error(errorMessage(t, sessionError));
        return false;
      } finally {
        setBusyId(null);
      }
    },
    [t, setBusyId]
  );

  const startReviewSession = useCallback(async () => {
    await openSession(async () => {
      const isOrphan = reviewCourseFilter.includes(ORPHAN_COURSE_FILTER);
      const courseIds = reviewCourseFilter.filter((value) => value !== ORPHAN_COURSE_FILTER);
      const fresh = await learningApi.listDueReviews(reviewSessionLimit, courseIds, {
        dueOnly: true,
        orphan: isOrphan,
        tags: reviewTagFilter,
      });
      setReviews(fresh);
      return fresh;
    }, 'learning.noReviews');
  }, [openSession, reviewCourseFilter, reviewTagFilter, reviewSessionLimit, setReviews]);

  const startCourseReviewSession = useCallback(
    async (courseId: string) => {
      // 课程复习专注本课程全部已入队卡片，不限于到期项，因此使用更大的队列上限
      await openSession(
        () => learningApi.listDueReviews(REVIEW_SESSION_QUEUE_LIMIT, courseId),
        'learning.noCourseReviews'
      );
    },
    [openSession]
  );

  /** u9 R3 软闸/报告薄弱集行动：开「节点集」范围的复习会话（后端 R2 多值
   * node_id 过滤，scoped 会话不下发全量队列）。due_only=false 早练语义——
   * 未到期卡同批出示且真实推进（due_count=0 非死态，当日未推进过的卡允许
   * 提前练一次）。不带课程过滤：供给节点可能跨课程（ready_partition 跨课
   * 程取教师），课程过滤会把跨课程供给卡静默滤空——复习队列按用户维度取
   * 卡，节点集合本身就是作用域。返回是否真正开窗（空队列只提示）。 */
  const startScopedReviewSession = useCallback(
    async (nodeIds: string[]): Promise<boolean> => {
      if (nodeIds.length === 0) return false;
      // 节点集会话与课程复习同款：专注该集合全部已入队卡片（含未到期），
      // 用更大的队列上限。
      return openSession(
        () =>
          learningApi.listDueReviews(REVIEW_SESSION_QUEUE_LIMIT, [], {
            dueOnly: false,
            nodeIds,
          }),
        'learning.noScopedReviews'
      );
    },
    [openSession]
  );

  return {
    sessionOpen,
    setSessionOpen,
    sessionQueue,
    setSessionQueue,
    answerReview,
    forgetReview,
    rateReview,
    skipReview,
    archiveReview,
    removeReview,
    markEditPending,
    startReviewSession,
    startCourseReviewSession,
    startScopedReviewSession,
  };
}
