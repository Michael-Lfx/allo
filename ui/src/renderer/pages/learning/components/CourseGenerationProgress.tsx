import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Card, Spin, Steps, Tag, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type {
  ILearningCourseGenerationEvent,
  ILearningGenerationToolCall,
} from '@/common/adapter/ipcBridge';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import type { CourseDetail, CourseKind } from '../types';
import {
  deriveModernStep,
  deriveStep,
  isModernPipeline,
  summarizeTools,
  triageCountsLine,
  uncertainTooMany,
} from '../model';
import { CourseSamplingLine } from './CourseSamplingLine';
import { LEGACY_VISIBLE_PHASES, MODERN_VISIBLE_PHASES, PHASE } from '../phaseMeta';

const { Text, Title } = Typography;




/** 课程生成过程视图：CreateCourseDialog 提交后就地展示。
 * 过程事件经 WS best-effort 推送（不重放不补发），事件流只是增强——
 * 终态一律以同步 HTTP 响应（status/result/error）为准。排队中（m7 资源闸）
 * 与运行中可取消（排队取消 = 释放排队位，body 零执行），关闭对话框不会
 * 终止生成（后台继续，右下角指示条可回到这里）。
 *
 * t5 新管线（知识库流：清点分诊 → 来源结构计划 → 分批展开）观察首帧
 * triage 阶段即切换新步骤条与读数行；描述流/旧客户端走旧五段，零回归。
 *
 * 学习图流（c1 最小修）：graph 建课 HTTP 秒回、结构生成在后台跑，WS 订阅
 * 随 status 翻 completed 即停用——v2 步骤条与活动流对该流是永不更新的冻结
 * 起始帧。running 只渲染单行后台提示（真实结构进度在工作区 structure 徽章/
 * 声明窗口）；queued 沿用排队提示；终态回到下方卡片/Alert 既有分支。
 *
 * 恢复态（c2 收敛出的终态）：failed/cancelled 不提供重发式重试——恢复态的
 * request 是按快照 topic 合成的（description = 后端 topic 标签），重发会以
 * 标签为名新建一门课程；改出本地化指引文案。in-session 终态重试不变。 */
export function CourseGenerationProgress({
  status,
  result,
  error,
  sources = [],
  courseKind,
  restored = false,
  structureLine = null,
  onStartLearning,
  onRetry,
  onCancel,
  pipeline,
}: {
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  result: CourseDetail | null;
  error: string | null;
  /** 多知识库（t4）：来源库名（「来源：库A、库B」；空 = 不展示） */
  sources?: string[];
  /** 课程类型（c1）：learning_graph 时呈现最小化（见头注释），缺省 = v2 流。 */
  courseKind?: CourseKind;
  /** 恢复态（页面挂载时从快照重建的生成任务）：终态面板只出指引文案，
   * 不提供重发式重试（见头注释）。缺省 = in-session。 */
  restored?: boolean;
  /** completed 卡片结构态行（v4）：结构在跑时「知识结构生成中 · 已 mm:ss」
   * 的已翻译文案（index 由 structurePillWatch 同一快照轮询派生，非本组件
   * 自建轮询）。null/缺省 = 不渲染（终态/排队/未接管都无行——卡片是 HTTP
   * 响应快照，本票不刷新它）；仅 learning_graph 卡片渲染。 */
  structureLine?: string | null;
  /** 建课管线代际（x11）：课程摘要可用时显式传入；生成中从 WS 帧兜底读取。 */
  pipeline?: 'sampling' | 'source_plan';
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

  // t5：声明字段优先（x11）——课程摘要或 WS 帧携带 pipeline；无字段时
  // 退回 triage 首帧嗅探（滚动部署期旧帧混入不炸）。
  const declaredPipeline = pipeline ?? events.find((event) => event.pipeline)?.pipeline;
  const modern = useMemo(
    () => isModernPipeline(events, declaredPipeline),
    [events, declaredPipeline]
  );
  const legacy = useMemo(() => deriveStep(events), [events]);
  const modernStep = useMemo(() => deriveModernStep(events), [events]);
  const running = status === 'running';
  // m7 资源闸排队中：任务已受理、等空闲许可——步骤条停在起点（wait），
  // 活动流无事件（生成循环尚未开跑），展示排队提示而非空转 spinner。
  const queued = status === 'queued';
  // 学习图流（c1 最小修）：呈现策略见头注释——v2 步骤条/活动流整段不渲染。
  const graphFlow = courseKind === 'learning_graph';
  // 「无可见事件」的判定不能是 events.length===0：循环开始等事件（如
  // started/generating）会被收进列表但渲染为空行，把等待提示挤掉。
  const visibleCount = events.filter((event) =>
    (modern ? MODERN_VISIBLE_PHASES : LEGACY_VISIBLE_PHASES).includes(event.phase as never)
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
      {!graphFlow && (
        <Steps
          size='small'
          current={stepIndex}
          status={
            running
              ? 'process'
              : queued
                ? 'wait'
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
      )}

      {/* 多知识库（t4）：来源披露——课程从哪几个库生成（首个 = 主来源）。
          分隔符随语言（zh-CN 顿号 / en-US 逗号），不写死中文标点。 */}
      {sources.length > 0 && (
        <Text type='secondary' className='text-12px'>
          {t('learning.courseSources', { names: sources.join(t('learning.listSeparator')) })}
        </Text>
      )}

      {/* 学习图流（c1 最小修）：running / queued 收口为单行——后台提示（真实
          结构进度在工作区 structure 徽章/声明窗口看）或排队提示，与取消入口
          同处一行；不叠加下方 v2 提示行（否则同屏双 spinner、排队提示渲染
          两遍）。终态不渲染本行——下方课程卡片与 Alert 既有分支接管。 */}
      {graphFlow && (running || queued) && (
        <div className='flex items-center justify-between gap-8px'>
          <div className='flex items-center gap-8px'>
            <Spin size={16} />
            <Text type='secondary'>
              {running
                ? t('learning.graphCreateBackgroundNotice')
                : t('learning.genQueuedNotice')}
            </Text>
          </div>
          <Button size='mini' status='danger' onClick={onCancel}>
            {t('learning.genCancel')}
          </Button>
        </div>
      )}

      {/* 实时活动流：round/audit 事件各占一行；无事件时退化为纯 spinner
          （学习图流整段不渲染，见上方 c1 注释） */}
      {!graphFlow && (
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
          {visibleCount === 0 && queued && (
            <div className='flex items-center justify-center gap-8px py-24px'>
              <Spin size={16} />
              <Text type='secondary'>{t('learning.genQueuedNotice')}</Text>
            </div>
          )}
          {events.map((event, index) => {
            if (event.phase === PHASE.started) {
              // c3 防御收口：started 行一律走词典，不读 event.text。started 帧
              // 带 text 的只有学习图流（后端硬编码文案，整段已随 c1 隐藏）；
              // v2/课时的 started 帧本就没有 text 字段。round 帧的 text 不同
              // ——那是模型叙述（nomifun-ai-agent learning_loop 产出的动态
              // 模型输出，非后端硬编码文案），下方照常渲染。
              return (
                <div key={index} className='text-13px text-t-secondary'>
                  {t('learning.genWaitingEvents')}
                </div>
              );
            }
            if (event.phase === PHASE.round) {
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
            if (event.phase === PHASE.audit) {
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
            if (event.phase === PHASE.triageCompleted && event.counts) {
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
            if (event.phase === PHASE.sourcePlanReady) {
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
            if (event.phase === PHASE.coverageReady) {
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
            if (event.phase === PHASE.publishing) {
              return (
                <div key={index} className='text-13px text-t-secondary'>
                  {t('learning.genPublishing')}
                </div>
              );
            }
            return null;
          })}
        </div>
      )}

      {/* v2 流的运行/排队提示行 + 取消入口（学习图流已在上方单行收口，
          不走这一行） */}
      {!graphFlow && (running || queued) && (
        <div className='flex items-center justify-between gap-8px'>
          <div className='flex items-center gap-8px'>
            <Spin size={16} />
            <Text type='secondary'>
              {running ? t('learning.genRunningHint') : t('learning.genQueuedNotice')}
            </Text>
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
              {/* 结构态披露（v4）：graph 流 HTTP 秒回后结构仍在后台跑——在跑
                  期间在卡片里如实披露（无交互纯文案，真话源头在 Pill/工作区）；
                  终态/排队/未接管不渲染（卡片是快照，本票不刷新它）。仅
                  learning_graph 渲染；v2 流卡片零变化。 */}
              {graphFlow && structureLine && (
                <div>
                  <Text type='secondary' className='text-12px'>
                    {structureLine}
                  </Text>
                </div>
              )}
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
              {/* 恢复态不提供重发式重试（合成请求会以后端 topic 标签为名
                  新建课程）：只出指引文案 */}
              {restored ? (
                <Text type='secondary' className='text-12px'>
                  {t('learning.restoredRetryGuidance')}
                </Text>
              ) : (
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
              )}
            </div>
          }
        />
      )}

      {/* 取消是中性终态而非失败：无错误详情；取消不保留草稿，重试即
          全新生成。恢复态同样不提供重发（见上），只出中性指引 */}
      {status === 'cancelled' && (
        <Alert
          type='warning'
          content={
            <div className='flex flex-col gap-8px'>
              <span>{t('learning.genCancelled')}</span>
              {restored ? (
                <Text type='secondary' className='text-12px'>
                  {t('learning.restoredRetryGuidance')}
                </Text>
              ) : (
                <div>
                  <Button size='small' type='primary' onClick={onRetry}>
                    {t('learning.genRetryFresh')}
                  </Button>
                </div>
              )}
            </div>
          }
        />
      )}
    </div>
  );
}
