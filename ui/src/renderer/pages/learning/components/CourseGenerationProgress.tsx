import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Card, Spin, Steps, Tag, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type {
  ILearningCourseGenerationEvent,
  ILearningGenerationToolCall,
} from '@/common/adapter/ipcBridge';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import type { CourseDetail } from '../types';
import {
  deriveModernStep,
  deriveStep,
  isModernPipeline,
  summarizeTools,
  triageCountsLine,
  uncertainTooMany,
} from '../model';
import { CourseSamplingLine } from './CourseSamplingLine';

const { Text, Title } = Typography;

/** 新管线帧里算「可见活动」的 phase（triage/source_plan 阶段有读数行或
 * 至少推进步骤条——不能让等待提示把它们挤掉）。 */
const MODERN_VISIBLE_PHASES = [
  'started',
  'triage_started',
  'triage_completed',
  'source_plan_started',
  'source_plan_ready',
  'coverage_ready',
  'round',
  'audit',
  'publishing',
] as const;




/** 课程生成过程视图：CreateCourseDialog 提交后就地展示。
 * 过程事件经 WS best-effort 推送（不重放不补发），事件流只是增强——
 * 终态一律以同步 HTTP 响应（status/result/error）为准。运行中可取消，
 * 关闭对话框不会终止生成（后台继续，右下角指示条可回到这里）。
 *
 * t5 新管线（知识库流：清点分诊 → 来源结构计划 → 分批展开）观察首帧
 * triage 阶段即切换新步骤条与读数行；描述流/旧客户端走旧五段，零回归。 */
export function CourseGenerationProgress({
  status,
  result,
  error,
  sources = [],
  onStartLearning,
  onRetry,
  onCancel,
}: {
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  result: CourseDetail | null;
  error: string | null;
  /** 多知识库（t4）：来源库名（「来源：库A、库B」；空 = 不展示） */
  sources?: string[];
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

  // t5：新管线（triage 首帧出现）切换新步骤条，其余路径零变化。
  const modern = useMemo(() => isModernPipeline(events), [events]);
  const legacy = useMemo(() => deriveStep(events), [events]);
  const modernStep = useMemo(() => deriveModernStep(events), [events]);
  const running = status === 'running';
  // 「无可见事件」的判定不能是 events.length===0：循环开始等事件（如
  // started/generating）会被收进列表但渲染为空行，把等待提示挤掉。
  const visibleCount = events.filter((event) =>
    (modern ? MODERN_VISIBLE_PHASES : ['round', 'audit', 'publishing', 'started']).includes(
      event.phase as never
    )
  ).length;
  const steps = modern
    ? [
        t('learning.genStepPrepare'),
        t('learning.genStepTriage'),
        t('learning.genStepSourcePlan'),
        t('learning.genStepExpansion'),
        t('learning.genStepDone'),
      ]
    : [t('learning.genStepPrepare'), t('learning.genStepOutline'), t('learning.genStepAudit')];
  let stepIndex: number;
  if (modern) {
    stepIndex = modernStep;
  } else {
    const { step, repairRounds, sawDanger } = legacy;
    // 修复段仅在审计不过后出现，避免空轮次误导
    if (repairRounds > 0 || sawDanger) steps.push(t('learning.genStepRepair'));
    steps.push(t('learning.genStepDone'));
    stepIndex = repairRounds > 0 || sawDanger ? step : step <= 3 ? Math.min(step, 2) : 4;
  }

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

      {/* 多知识库（t4）：来源披露——课程从哪几个库生成（首个 = 主来源）。
          分隔符随语言（zh-CN 顿号 / en-US 逗号），不写死中文标点。 */}
      {sources.length > 0 && (
        <Text type='secondary' className='text-12px'>
          {t('learning.courseSources', { names: sources.join(t('learning.listSeparator')) })}
        </Text>
      )}

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
          if (event.phase === 'triage_completed' && event.counts) {
            const counts = event.counts;
            return (
              <div key={index} className='flex flex-col gap-4px text-13px leading-20px'>
                <span className='font-500 text-t-primary'>{triageCountsLine(counts, t)}</span>
                {event.degraded && (
                  <span className='text-warning-6'>· {t('learning.genTriageDegraded')}</span>
                )}
                {uncertainTooMany(counts) && (
                  <span className='text-warning-6'>
                    · {t('learning.genUncertainTooMany', { count: counts.uncertain })}
                  </span>
                )}
                {/* 可展开明细：跳过理由 / duplicate_of / uncertain（帧里有界 ≤10） */}
                {(event.excluded?.length ?? 0) + (event.duplicates?.length ?? 0) + (event.uncertain?.length ?? 0) > 0 && (
                  <NomiCollapse bordered={false}>
                    {(event.excluded?.length ?? 0) > 0 && (
                      <NomiCollapse.Item
                        name={`triage-excluded-${index}`}
                        header={t('learning.genTriageExcludedTitle')}
                      >
                        {event.excluded?.map((entry) => (
                          <Text key={entry.path} type='secondary' className='block break-all text-12px'>
                            · {entry.path} — {entry.reason}
                          </Text>
                        ))}
                      </NomiCollapse.Item>
                    )}
                    {(event.duplicates?.length ?? 0) > 0 && (
                      <NomiCollapse.Item
                        name={`triage-duplicates-${index}`}
                        header={t('learning.genTriageDuplicatesTitle')}
                      >
                        {event.duplicates?.map((duplicate) => (
                          <Text key={duplicate.path} type='secondary' className='block break-all text-12px'>
                            · {duplicate.path} → {duplicate.duplicate_of}
                          </Text>
                        ))}
                      </NomiCollapse.Item>
                    )}
                    {(event.uncertain?.length ?? 0) > 0 && (
                      <NomiCollapse.Item
                        name={`triage-uncertain-${index}`}
                        header={t('learning.genTriageUncertainTitle')}
                      >
                        {event.uncertain?.map((entry) => (
                          <Text key={entry.path} type='secondary' className='block break-all text-12px'>
                            · {entry.path} — {entry.reason}
                          </Text>
                        ))}
                      </NomiCollapse.Item>
                    )}
                  </NomiCollapse>
                )}
              </div>
            );
          }
          if (event.phase === 'source_plan_ready') {
            return (
              <div key={index} className='text-13px leading-20px'>
                <span className='font-500 text-t-primary'>
                  {t('learning.genSourcePlanReady', {
                    chapters: event.chapters ?? 0,
                    concepts: event.concepts ?? 0,
                  })}
                </span>
                {(event.gaps?.length ?? 0) > 0 && (
                  <span className='ml-8px text-t-secondary'>
                    · {t('learning.sourceCoverageGaps')} {event.gaps?.length}
                  </span>
                )}
              </div>
            );
          }
          if (event.phase === 'coverage_ready') {
            return (
              <div key={index} className='text-13px leading-20px'>
                <span className='font-500 text-t-primary'>
                  {t('learning.genCoverageReady', {
                    covered: event.topics_covered ?? 0,
                    skipped: event.topics_skipped ?? 0,
                    fallback: event.fallback ?? 0,
                  })}
                </span>
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
              {/* 采样披露（t2）：建课完成视图如实报出按目标采样了多少文件，
                  目标与库无交集时明说退回默认顺序（工作区同一行、同一口径） */}
              {result.course.sampling && (
                <div>
                  <CourseSamplingLine sampling={result.course.sampling} />
                </div>
              )}
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
