/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Card, Typography } from '@arco-design/web-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { learningApi } from '../api';
import type { QuizProgressView } from '../types';
import { errorMessage } from '../utils';

const { Text } = Typography;

/** 测验入口卡（u2）：课程详情页的测验区块——进度读数 N/K + 开测按钮。
 *
 * 呈现形态裁决（票面授权二选一）：独立 Card 区块，照课程详情页既有卡片
 * 层次（recommendedLesson 卡的「左文案右按钮」形态）——「进度卡内嵌」无
 * 宿主可落（课时进度条已移入左侧大纲栏），塞大纲栏即第三种形态。位置
 * 固定在推荐课时卡之后：积累循环（学新 → 结算）优先、检测次之，且不随
 * due 摆位（可预期性）。
 *
 * 刷新时机（R2，不轮询）：进页挂载拉一次；refreshToken 变化（= 会话结束
 * 回调后 index.tsx bump 令牌）再拉一次。读数面非门禁，结算后由会话结束
 * 回调驱动归零呈现，轮询徒增请求面。
 *
 * 409 兜底（R3）：读数非门禁——due 呈现与点开测之间存在竞态窗口，开测
 * 被后端 409 拒时（onStartQuiz 回调拒绝即本区块的兜底触发面，u3 接线后
 * 生效）toast 报实情 + 重拉进度；会话容器内的完整边界态归 u5。 */
export function QuizEntryCard({
  courseId,
  refreshToken,
  onStartQuiz,
}: {
  courseId: string;
  /** 进度刷新令牌：进页初值；测验会话结束回调后 +1（u3 接线）驱动重拉 */
  refreshToken: number;
  /** u3 接线位：打开会话容器。回调拒绝（如开测 409）= 本区块兜底呈现 */
  onStartQuiz: () => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const [progress, setProgress] = useState<QuizProgressView | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [starting, setStarting] = useState(false);
  // 并发重拉 last-wins 序号：令牌 bump / 409 兜底重拉与进页拉并行时，
  // 只让最新一次写读数，旧响应不得回写陈旧 N/K。
  const pullSeqRef = useRef(0);

  const pullProgress = useCallback(async () => {
    const seq = ++pullSeqRef.current;
    try {
      const view = await learningApi.quizProgress(courseId);
      if (seq === pullSeqRef.current) {
        setProgress(view);
        setLoadFailed(false);
      }
    } catch {
      // 读数拉取失败：降级呈现失败句 + 按钮禁用，不渲染空、不静默
      if (seq === pullSeqRef.current) setLoadFailed(true);
    }
  }, [courseId]);

  // R2 刷新时机：进页（挂载）+ 令牌变化各拉一次；无定时器即无轮询面
  useEffect(() => {
    void pullProgress();
  }, [pullProgress, refreshToken]);

  const due = progress?.due ?? false;
  const handleStart = () => {
    if (!due) return;
    setStarting(true);
    Promise.resolve()
      .then(onStartQuiz)
      .catch((startError: unknown) => {
        // R3 兜底：报实情（errorMessage 409 → conflictBusy 单源映射）+
        // 重拉读数（开测被拒瞬间 N/K 可能已变，呈现如实）。会话容器内的
        // 完整边界态归 u5。
        Message.error(errorMessage(t, startError));
        void pullProgress();
      })
      .finally(() => setStarting(false));
  };

  // 进度在拉：先不出卡，避免读数空闪；存在性语义（归档/删除不渲染）在
  // CourseWorkspace 挂点裁决
  if (!progress && !loadFailed) return null;

  return (
    <Card>
      <div className='flex flex-wrap items-center justify-between gap-12px'>
        <div className='min-w-0'>
          <div className='font-600'>{t('learning.quizSessionTitle')}</div>
          {loadFailed || !progress ? (
            <Text type='secondary'>{t('learning.quizLoadFailed')}</Text>
          ) : (
            <>
              <Text type='secondary'>
                {t('learning.quizProgressReading', {
                  settled: progress.settled_count,
                  trigger: progress.trigger_count,
                })}
              </Text>
              {!due && (
                <div>
                  <Text type='secondary' className='text-12px'>
                    {t('learning.quizNotDueHint', { trigger: progress.trigger_count })}
                  </Text>
                </div>
              )}
            </>
          )}
        </div>
        <Button type='primary' loading={starting} disabled={!due} onClick={handleStart}>
          {t('learning.quizStartButton')}
        </Button>
      </div>
    </Card>
  );
}
