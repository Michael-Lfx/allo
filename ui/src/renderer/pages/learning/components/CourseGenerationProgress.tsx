import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Card, Spin, Steps, Tag, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type {
  ILearningCourseGenerationEvent,
  ILearningGenerationToolCall,
} from '@/common/adapter/ipcBridge';
import type { CourseDetail } from '../types';
import { deriveStep, summarizeTools } from '../model';

const { Text, Title } = Typography;



/** 课程生成过程视图：CreateCourseDialog 提交后就地展示。
 * 过程事件经 WS best-effort 推送（不重放不补发），事件流只是增强——
 * 终态一律以同步 HTTP 响应（status/result/error）为准。运行中可取消，
 * 关闭对话框不会终止生成（后台继续，右下角指示条可回到这里）。 */
export function CourseGenerationProgress({
  status,
  result,
  error,
  onStartLearning,
  onRetry,
  onCancel,
}: {
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  result: CourseDetail | null;
  error: string | null;
  onStartLearning: (courseId: string) => void;
  onRetry: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [events, setEvents] = useState<ILearningCourseGenerationEvent[]>([]);
  const streamRef = useRef<HTMLDivElement>(null);

  // 仅运行中订阅过程事件；HTTP 终态（completed/failed/cancelled）后停止消费，
  // 已收到的活动流保留用于回看
  useEffect(() => {
    if (status !== 'running') return;
    return ipcBridge.learning.courseGeneration.on((event) => {
      setEvents((prev) => [...prev.slice(-99), event]);
    });
  }, [status]);

  // 新事件到达时滚动到底部，保持最新活动可见
  useEffect(() => {
    const stream = streamRef.current;
    if (stream) stream.scrollTop = stream.scrollHeight;
  }, [events]);

  const { step, repairRounds, sawDanger } = useMemo(() => deriveStep(events), [events]);
  const running = status === 'running';
  // 「无可见事件」的判定不能是 events.length===0：循环开始等事件（如
  // started/generating）会被收进列表但渲染为空行，把等待提示挤掉。
  const visibleCount = events.filter((event) =>
    ['round', 'audit', 'publishing', 'started'].includes(event.phase),
  ).length;
  const steps = [t('learning.genStepPrepare'), t('learning.genStepOutline'), t('learning.genStepAudit')];
  // 修复段仅在审计不过后出现，避免空轮次误导
  if (repairRounds > 0 || sawDanger) steps.push(t('learning.genStepRepair'));
  steps.push(t('learning.genStepDone'));
  const stepIndex = repairRounds > 0 || sawDanger ? step : step <= 3 ? Math.min(step, 2) : 4;

  return (
    <div className='flex flex-col gap-16px'>
      <Steps
        size='small'
        current={stepIndex}
        status={
          running
            ? 'process'
            : status === 'failed'
              ? 'error'
              : status === 'cancelled'
                ? 'wait'
                : 'finish'
        }
      >
        {steps.map((label) => (
          <Steps.Step key={label} title={label} />
        ))}
      </Steps>

      {/* 实时活动流：round/audit 事件各占一行；无事件时退化为纯 spinner */}
      <div
        ref={streamRef}
        className='flex max-h-260px min-h-120px flex-col gap-6px overflow-y-auto rounded-8px bg-[var(--color-fill-1)] p-12px'
      >
        {visibleCount === 0 && running && (
          <div className='flex items-center justify-center gap-8px py-24px'>
            <Spin size={16} />
            <Text type='secondary'>{t('learning.genWaitingEvents')}</Text>
          </div>
        )}
        {events.map((event, index) => {
          if (event.phase === 'started') {
            return (
              <div key={index} className='text-13px text-t-secondary'>
                {event.text ?? t('learning.genWaitingEvents')}
              </div>
            );
          }
          if (event.phase === 'round') {
            const repair = event.loop === 'repair';
            const roundLabel = repair
              ? t('learning.genRepairRoundLabel', {
                  round: event.round ?? index + 1,
                  max: event.max_rounds ?? '',
                })
              : t('learning.genRoundLabel', { round: event.round ?? index + 1 });
            const tools = summarizeTools(event.tools);
            return (
              <div key={index} className='text-13px leading-20px'>
                <span className='font-500 text-t-primary'>{roundLabel}</span>
                {tools && <span className='ml-8px text-t-secondary'>{tools}</span>}
                {event.text && (
                  <div className='mt-2px break-all text-t-secondary'>
                    {event.text.slice(0, 140)}
                  </div>
                )}
              </div>
            );
          }
          if (event.phase === 'audit') {
            const danger = event.danger ?? 0;
            return (
              <div key={index} className='flex flex-col gap-4px text-13px leading-20px'>
                <div className='flex flex-wrap items-center gap-6px'>
                  <span className='font-500 text-t-primary'>{t('learning.genAuditLabel')}</span>
                  {danger > 0 && (
                    <Tag size='small' color='red' className='!mx-0'>
                      {t('learning.genDangerCount', { count: danger })}
                    </Tag>
                  )}
                  {(event.warning ?? 0) > 0 && (
                    <Tag size='small' color='orange' className='!mx-0'>
                      {t('learning.genWarningCount', { count: event.warning })}
                    </Tag>
                  )}
                  {(event.info ?? 0) > 0 && (
                    <Tag size='small' color='gray' className='!mx-0'>
                      {t('learning.genInfoCount', { count: event.info })}
                    </Tag>
                  )}
                </div>
                {(event.top ?? []).map((finding, findingIndex) => (
                  <div key={findingIndex} className='break-all text-t-secondary'>
                    {danger > 0 ? (
                      <span className='text-danger-6'>· {finding}</span>
                    ) : (
                      <span>· {finding}</span>
                    )}
                  </div>
                ))}
              </div>
            );
          }
          if (event.phase === 'publishing') {
            return (
              <div key={index} className='text-13px text-t-secondary'>
                {t('learning.genPublishing')}
              </div>
            );
          }
          return null;
        })}
      </div>

      {running && (
        <div className='flex items-center justify-between gap-8px'>
          <div className='flex items-center gap-8px'>
            <Spin size={16} />
            <Text type='secondary'>{t('learning.genRunningHint')}</Text>
          </div>
          <Button size='mini' status='danger' onClick={onCancel}>
            {t('learning.genCancel')}
          </Button>
        </div>
      )}

      {status === 'completed' && result && (
        <Card size='small'>
          <div className='flex flex-wrap items-center justify-between gap-12px'>
            <div className='min-w-0'>
              <Title heading={6} className='!m-0'>
                {result.course.title}
              </Title>
              <Text type='secondary'>
                {t('learning.genModulesCount', { count: result.modules.length })} ·{' '}
                {t('learning.genLessonsCount', { count: result.course.total_lessons })}
              </Text>
            </div>
            <Button type='primary' onClick={() => onStartLearning(result.course.id)}>
              {t('learning.genStartLearning')}
            </Button>
          </div>
        </Card>
      )}

      {status === 'failed' && (
        <Alert
          type='error'
          content={
            <div className='flex flex-col gap-8px'>
              <span>{t('learning.genFailed')}</span>
              {/* 失败详情可能携带后端附加的草稿审计长文，而 Modal 内容区
                  不可滚动：限高滚动保证「重试」按钮始终首屏可见 */}
              {error && (
                <div className='max-h-160px overflow-y-auto whitespace-pre-wrap break-all rounded-8px bg-[var(--color-fill-1)] p-8px text-13px'>
                  {error}
                </div>
              )}
              <div className='flex flex-col gap-4px'>
                <div>
                  <Button size='small' type='primary' onClick={onRetry}>
                    {t('learning.genRetryResume')}
                  </Button>
                </div>
                <Text type='secondary' className='text-12px'>
                  {t('learning.genRetryResumeHint')}
                </Text>
              </div>
            </div>
          }
        />
      )}

      {/* 取消是中性终态而非失败：无错误详情；取消不保留草稿，重试即
          全新生成 */}
      {status === 'cancelled' && (
        <Alert
          type='warning'
          content={
            <div className='flex flex-col gap-8px'>
              <span>{t('learning.genCancelled')}</span>
              <div>
                <Button size='small' type='primary' onClick={onRetry}>
                  {t('learning.genRetryFresh')}
                </Button>
              </div>
            </div>
          }
        />
      )}
    </div>
  );
}
