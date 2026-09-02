import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Card, Spin, Steps, Tag, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type {
  ILearningCourseGenerationEvent,
  ILearningGenerationToolCall,
} from '@/common/adapter/ipcBridge';
import type { CourseDetail } from '../types';

const { Text, Title } = Typography;

/** 步骤条四段：准备 → 构建大纲 → 审计 → 修复（仅审计不过时出现）→ 完成 */
type GenStep = 0 | 1 | 2 | 3 | 4;

/** 把已收到的事件流折叠成步骤条状态：修复轮数与是否出现过 DANGER 审计 */
function deriveStep(events: ILearningCourseGenerationEvent[]): {
  step: GenStep;
  repairRounds: number;
  sawDanger: boolean;
} {
  let step: GenStep = 0;
  let repairRounds = 0;
  let sawDanger = false;
  for (const event of events) {
    if (event.phase === 'round' && event.loop === 'repair') {
      step = Math.max(step, 3) as GenStep;
      repairRounds += 1;
    } else if (event.phase === 'audit') {
      step = Math.max(step, 2) as GenStep;
      if ((event.danger ?? 0) > 0) sawDanger = true;
    } else if (event.phase === 'round' && event.loop === 'generate') {
      step = Math.max(step, 1) as GenStep;
    } else if (event.phase === 'publishing' || event.phase === 'completed') {
      step = Math.max(step, 4) as GenStep;
    }
  }
  // 事件流缺失（WS 未连接/丢帧）时退化为构建大纲进行中，只转 spinner
  if (events.length === 0) step = 1;
  return { step, repairRounds, sawDanger };
}

/** 一轮工具调用聚合成 `co_patch ×8 ✓ · co_query ✗` 形态 */
function summarizeTools(tools: ILearningGenerationToolCall[] | undefined): string {
  const counts = new Map<string, { ok: number; failed: number }>();
  for (const call of tools ?? []) {
    const entry = counts.get(call.name) ?? { ok: 0, failed: 0 };
    if (call.is_error) entry.failed += 1;
    else entry.ok += 1;
    counts.set(call.name, entry);
  }
  return [...counts.entries()]
    .map(([name, { ok, failed }]) => {
      const times = ok + failed > 1 ? ` ×${ok + failed}` : '';
      const mark = failed > 0 ? ' ✗' : ' ✓';
      return `${name}${times}${mark}`;
    })
    .join(' · ');
}

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
  status: 'running' | 'completed' | 'failed';
  result: CourseDetail | null;
  error: string | null;
  onStartLearning: (courseId: string) => void;
  onRetry: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [events, setEvents] = useState<ILearningCourseGenerationEvent[]>([]);
  const streamRef = useRef<HTMLDivElement>(null);

  // 仅运行中订阅过程事件；HTTP 终态（completed/failed）后停止消费，
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
  const steps = [t('learning.genStepPrepare'), t('learning.genStepOutline'), t('learning.genStepAudit')];
  // 修复段仅在审计不过后出现，避免空轮次误导
  if (repairRounds > 0 || sawDanger) steps.push(t('learning.genStepRepair'));
  steps.push(t('learning.genStepDone'));
  const stepIndex = repairRounds > 0 || sawDanger ? step : step <= 3 ? Math.min(step, 2) : 4;

  return (
    <div className='flex flex-col gap-16px'>
      <Steps size='small' current={stepIndex} status={running ? 'process' : status === 'failed' ? 'error' : 'finish'}>
        {steps.map((label) => (
          <Steps.Step key={label} title={label} />
        ))}
      </Steps>

      {/* 实时活动流：round/audit 事件各占一行；无事件时退化为纯 spinner */}
      <div
        ref={streamRef}
        className='flex max-h-260px min-h-120px flex-col gap-6px overflow-y-auto rounded-8px bg-[var(--color-fill-1)] p-12px'
      >
        {events.length === 0 && running && (
          <div className='flex items-center justify-center gap-8px py-24px'>
            <Spin size={16} />
            <Text type='secondary'>{t('learning.genWaitingEvents')}</Text>
          </div>
        )}
        {events.map((event, index) => {
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
                      <span className='text-[rgb(var(--danger-6))]'>· {finding}</span>
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
              <div>
                <Button size='small' type='primary' onClick={onRetry}>
                  {t('learning.genRetry')}
                </Button>
              </div>
            </div>
          }
        />
      )}
    </div>
  );
}
