/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  Alert,
  Button,
  Empty,
  Modal,
  Spin,
  Table,
  Tabs,
  Tag,
  Typography,
} from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { lessonStatusTagColors } from '../model';
import { contentColumnWidth, STANDARD_GRAPH_MAX_WIDTH } from '../layout';
import { statusLabel } from '../utils';
import { learningApi } from '../api';
import { errorMessage } from '../utils';
import type {
  Activity,
  AttemptRecord,
  CourseDetail,
  GraphBatchView,
  GraphConceptRowView,
  LearningStructureView,
  Lesson,
  LessonStatus,
} from '../types';
import { LessonBlock } from './LessonStudy';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import ContentWidthToggle, { useWideContentLayout } from './ContentWidthToggle';

const { Text, Title, Paragraph } = Typography;

/**
 * 学习图课程工作区（beta，概念网 + 生长模型 ADR-0009）：
 * - 顶条：知识结构状态（生成中 / 已生成 / 失败重试）+ 手动补充节点
 *   （就绪 x/7）；
 * - 四个视图：学习（就绪集导航 + 节点课时内容）、学习记录（批次时间线，
 *   生长史即课程史）、概念表（概念 × 教/假定 × 档位 × 跨课程来源）、
 *   结构图（知识结构读侧投影，后续落地）。
 * 没有先修边、没有锁定态——发布即就绪。
 */
const LearningGraphWorkspace: React.FC<{
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onBack: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
  onGenerate: (lesson: Lesson) => void;
  onRefresh: () => void;
}> = ({
  detail,
  busyId,
  attemptResults,
  onBack,
  onProgress,
  onAttempt,
  onGenerate,
  onRefresh,
}) => {
  const { t } = useTranslation();
  const model = useLearningAutogenModel();
  const { wide: wideContent } = useWideContentLayout();
  const graph = detail.graph;
  const [activeTab, setActiveTab] = useState('study');

  const lessons = useMemo(
    () => detail.modules.flatMap((module) => module.lessons),
    [detail.modules]
  );
  const lessonsById = useMemo(
    () => new Map(lessons.map((lesson) => [lesson.id, lesson])),
    [lessons]
  );

  // 后台结构生成/生长的 WS 事件触发刷新：结构启动翻出「生成中」徽章、
  // 画完立即上屏、终结（完成/失败/取消）落定状态——思考化之后结构与生长
  // 都要跑分钟级，不刷新用户会以为什么都没发生。
  useEffect(() => {
    return ipcBridge.learning.courseGeneration.on((event) => {
      if (
        event.course_id === detail.course.id &&
        (event.event === 'growth_started' ||
          event.event === 'growth_completed' ||
          event.event === 'growth_failed' ||
          event.event === 'growth_cancelled' ||
          event.event === 'structure_started' ||
          event.event === 'structure_completed' ||
          event.event === 'structure_failed')
      ) {
        onRefresh();
      }
    });
  }, [detail.course.id, onRefresh]);

  const [selectedLessonId, setSelectedLessonId] = useState<string | null>(
    () => graph?.recommended[0] ?? null
  );

  const isRecommended = useCallback(
    (lessonId: string) => graph?.recommended.includes(lessonId) ?? false,
    [graph]
  );
  const primaryReadyId = graph?.recommended[0] ?? null;
  const primaryLesson = primaryReadyId ? lessonsById.get(primaryReadyId) ?? null : null;

  // 完成节点后推荐首位会推进：内容区自动跟随到下一个应学节点；
  // 用户手动点选不被覆盖（与普通课程工作区的跟随策略一致）。
  const lastRecommendedIdRef = React.useRef<string | null>(primaryReadyId);
  useEffect(() => {
    if (primaryReadyId && primaryReadyId !== lastRecommendedIdRef.current) {
      lastRecommendedIdRef.current = primaryReadyId;
      setSelectedLessonId(primaryReadyId);
    }
  }, [primaryReadyId]);

  // 选中的课时在前端数据里消失时（生长后课程刷新）回退到推荐首位
  const selectedLesson = useMemo(
    () =>
      (selectedLessonId ? lessonsById.get(selectedLessonId) : null) ??
      (primaryReadyId ? lessonsById.get(primaryReadyId) : null) ??
      null,
    [lessonsById, selectedLessonId, primaryReadyId]
  );

  // ── 知识结构状态（读侧投影 + 生成中/失败态）─────────────────────────────
  const [structure, setStructure] = useState<LearningStructureView | null>(null);
  const [structureError, setStructureError] = useState<string | null>(null);
  const [structureRunning, setStructureRunning] = useState(false);
  const [regenerating, setRegenerating] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphStructure(detail.course.id)
        .then((view) => {
          if (cancelled) return;
          setStructure(view);
          setStructureError(null);
          if (view.structure) setStructureRunning(false);
        })
        .catch((loadError) => {
          if (!cancelled) {
            setStructureError(loadError instanceof Error ? loadError.message : String(loadError));
          }
        });
    };
    load();
    // 结构生成的开始/终结帧驱动「生成中」状态机，终结即重取投影。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== detail.course.id) return;
      if (event.event === 'structure_started') setStructureRunning(true);
      if (event.event === 'structure_completed' || event.event === 'structure_failed') load();
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [detail.course.id]);

  const regenerateStructure = useCallback(async () => {
    setRegenerating(true);
    try {
      setStructureRunning(true);
      await learningApi.regenerateGraphStructure(detail.course.id, {
        provider_id: model.choice?.provider_id,
        model: model.choice?.model,
      });
    } catch (actionError) {
      setStructureRunning(false);
      Modal.error({
        title: t('learning.learningGraphStructureFailed'),
        content: errorMessage(t, actionError),
      });
    } finally {
      setRegenerating(false);
    }
  }, [detail.course.id, model.choice, t]);

  // ── 手动补充节点 ────────────────────────────────────────────────────────
  const [growing, setGrowing] = useState(false);
  const grow = useCallback(async () => {
    setGrowing(true);
    try {
      await learningApi.growGraph(detail.course.id);
      onRefresh();
    } catch (actionError) {
      Modal.error({ title: t('learning.learningGraphGrowFailed'), content: errorMessage(t, actionError) });
    } finally {
      setGrowing(false);
    }
  }, [detail.course.id, onRefresh, t]);

  if (!graph) {
    return <Empty description={t('learning.learningGraphEmpty')} />;
  }
  const readyCount = graph.ready_count;
  const readyStockFull = readyCount >= graph.ready_target;

  return (
    <div className='app-page-shell h-full w-full box-border overflow-y-auto'>
      <div
        className={`mx-auto flex h-full w-full flex-col gap-10px ${contentColumnWidth(wideContent, STANDARD_GRAPH_MAX_WIDTH)}`}
      >
      {/* 头部：返回 + 标题/Beta + 学习目标 + 模型选择（对齐传统课程工作区） */}
      <Button type='text' className='self-start !px-0' onClick={onBack}>
        {t('learning.back')}
      </Button>
      <div className='flex flex-wrap items-start justify-between gap-12px'>
        <div className='min-w-0 flex-1'>
          <div className='flex items-center gap-8px'>
            <Title heading={3} className='!m-0'>
              {detail.course.title}
            </Title>
            <Tag size='small' color='orangered' className='!mx-0 shrink-0'>
              {t('learning.learningGraphBeta')}
            </Tag>
          </div>
          {graph.goal.trim() !== '' && (
            <Paragraph className='!mb-0 !mt-4px text-t-secondary'>{graph.goal}</Paragraph>
          )}
        </div>
        <div className='flex shrink-0 items-center gap-8px'>
          <ContentWidthToggle />
          <LearningModelSelector
            choice={model.choice}
            onChange={(choice) => void model.setChoice(choice)}
            size='small'
          />
        </div>
      </div>

      {/* 知识结构状态 + 补充节点（生长控制条） */}
      <div className='flex flex-col gap-8px rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'>
        <div className='flex flex-wrap items-center gap-8px'>
          <Text bold className='text-13px'>
            {t('learning.learningGraphStructureTitle')}
          </Text>
          {structureError ? (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureLoadFailed')}
            </Text>
          ) : structure === null ? (
            <Spin size={12} />
          ) : structureRunning ? (
            <Tag size='small' color='processing' className='!mx-0'>
              {t('learning.learningGraphStructureGenerating')}
            </Tag>
          ) : structure.structure ? (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureSummary', {
                spines: structure.structure.spines.length,
                blocks: structure.structure.blocks.length,
                doors: structure.terrain?.doors.length ?? 0,
              })}
              {structure.generated_at
                ? ` · ${new Date(structure.generated_at).toLocaleString()}`
                : ''}
            </Text>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureMissing')}
            </Text>
          )}
          <Button
            size='mini'
            type='text'
            loading={regenerating}
            disabled={regenerating || structureRunning || busyId !== null}
            onClick={() => void regenerateStructure()}
          >
            {structure?.structure
              ? t('learning.learningGraphStructureRegenerate')
              : t('learning.learningGraphStructureGenerate')}
          </Button>
          <span className='flex-1' />
          {graph.growth_running && (
            <Tag size='small' color='processing' className='!mx-0'>
              {t('learning.learningGraphGrowing')}
            </Tag>
          )}
          <Button
            size='small'
            type='primary'
            loading={growing}
            disabled={growing || busyId !== null}
            onClick={() => void grow()}
          >
            {readyStockFull
              ? t('learning.learningGraphGrowReady')
              : t('learning.learningGraphGrow', {
                  ready: readyCount,
                  target: graph.ready_target,
                })}
          </Button>
        </div>
        {/* 尚未生成时的说明（生成按钮在上一行；生成中不显示） */}
        {!structureRunning && structure !== null && !structure.structure && !structureError && (
          <Text type='secondary' className='text-12px'>
            {t('learning.learningGraphStructureHint')}
          </Text>
        )}
        {/* R 软闸建议条：结构就绪但供给节点已遗忘（ADR-0009 Amendment 1） */}
        {graph.blocked.map((blocked) => (
          <Alert
            key={blocked.lesson_id}
            type='warning'
            content={t('learning.learningGraphBlockedAdvice', {
              title: blocked.title,
              supplier: blocked.supplier_title,
              percent: Math.round(blocked.r * 100),
              due: blocked.due_count,
            })}
          />
        ))}
      </div>

      <Tabs activeTab={activeTab} onChange={setActiveTab} type='line' className='flex min-h-0 flex-1 flex-col [&_.arco-tabs-content]:flex-1 [&_.arco-tabs-content]:pt-8px'>
        <Tabs.TabPane key='study' title={t('learning.learningGraphTabStudy')}>
          <div className='flex min-h-0 gap-12px'>
            <aside className='flex w-280px shrink-0 flex-col gap-10px'>
              {/* 主推荐卡：当前应学的节点——最重要的学习入口，点击即进入学习 */}
              <div
                className='cursor-pointer rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px transition-colors hover:border-[var(--color-primary-6)]'
                onClick={() => primaryReadyId && setSelectedLessonId(primaryReadyId)}
              >
                <Text bold className='text-13px'>
                  {t('learning.learningGraphContinueTitle')}
                </Text>
                {primaryReadyId ? (
                  <>
                    <div className='mt-6px text-14px font-600 leading-20px text-[var(--color-text-1)]'>
                      {primaryLesson?.title}
                    </div>
                    <div className='mt-4px text-11px text-t-tertiary'>
                      {t('learning.learningGraphNodeMinutes', {
                        min: primaryLesson?.estimated_minutes ?? 10,
                      })}
                    </div>
                    {primaryLesson && !primaryLesson.generated && (
                      <div className='mt-10px'>
                        <Button
                          type='primary'
                          size='small'
                          loading={busyId === primaryLesson.id}
                          onClick={(event) => {
                            event.stopPropagation();
                            onGenerate(primaryLesson);
                          }}
                        >
                          {t('learning.learningGraphGenerateContent')}
                        </Button>
                      </div>
                    )}
                  </>
                ) : (
                  <Empty description={t('learning.learningGraphReadyEmpty')} />
                )}
              </div>
              {/* 其他可学节点 */}
              {graph.recommended.slice(1).map((lessonId) => {
                const lesson = lessonsById.get(lessonId);
                if (!lesson) return null;
                return (
                  <button
                    key={lessonId}
                    type='button'
                    onClick={() => setSelectedLessonId(lessonId)}
                    className={`rd-6px flex cursor-pointer items-center justify-between gap-8px border-none bg-transparent px-6px py-6px text-left font-inherit text-13px transition-colors hover:bg-[var(--color-fill-1)] ${
                      selectedLessonId === lessonId
                        ? 'bg-primary-1 font-500 text-primary-6'
                        : 'text-[var(--color-text-1)]'
                    }`}
                  >
                    <span className='min-w-0 truncate'>{lesson.title}</span>
                    <span className='shrink-0 text-11px text-t-tertiary'>
                      {t('learning.learningGraphNodeMinutes', { min: lesson.estimated_minutes })}
                    </span>
                  </button>
                );
              })}
            </aside>

            {/* 中央内容区：选中节点的完整课时学习（正文/练习题/完成/追加练习） */}
            <section className='flex min-w-0 flex-1 flex-col overflow-hidden rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
              {!selectedLesson ? (
                <div className='flex h-full flex-1 items-center justify-center'>
                  <Empty description={t('learning.learningGraphContentEmpty')} />
                </div>
              ) : (
                <div className='h-full overflow-y-auto p-16px'>
                  <div className='flex flex-wrap items-center gap-8px'>
                    <Title heading={4} className='!m-0'>
                      {selectedLesson.title}
                    </Title>
                    <Tag
                      size='small'
                      color={lessonStatusTagColors[selectedLesson.status]}
                      className='!mx-0'
                    >
                      {statusLabel(selectedLesson.status, t)}
                    </Tag>
                    {isRecommended(selectedLesson.id) && (
                      <Tag size='small' color='gold' className='!mx-0'>
                        {t('learning.learningGraphRecommendedShort')}
                      </Tag>
                    )}
                    <span className='flex-1' />
                    <Button
                      size='small'
                      disabled={busyId !== null}
                      onClick={() =>
                        onProgress(
                          selectedLesson,
                          selectedLesson.status === 'skipped' ? 'not_started' : 'skipped'
                        )
                      }
                    >
                      {selectedLesson.status === 'skipped'
                        ? t('learning.learningGraphUnskip')
                        : t('learning.learningGraphSkip')}
                    </Button>
                  </div>
                  <div className='mt-12px'>
                    <LessonBlock
                      lesson={selectedLesson}
                      sourceKbId={detail.course.source_kb_id}
                      busyId={busyId}
                      attemptResults={attemptResults}
                      onProgress={onProgress}
                      onAttempt={onAttempt}
                      onGenerate={(target) => onGenerate(target)}
                      onRefresh={onRefresh}
                    />
                  </div>
                </div>
              )}
            </section>
          </div>
        </Tabs.TabPane>
        <Tabs.TabPane key='history' title={t('learning.learningGraphTabHistory')}>
          <GraphHistory courseId={detail.course.id} enrollmentKey={detail.enrollment_id} />
        </Tabs.TabPane>
        <Tabs.TabPane key='concepts' title={t('learning.learningGraphTabConcepts')}>
          <ConceptTable courseId={detail.course.id} />
        </Tabs.TabPane>
      </Tabs>
      </div>
    </div>
  );
};

/** 学习记录：批次时间线（倒序）——生长史即课程史。 */
const GraphHistory: React.FC<{ courseId: string; enrollmentKey: string | null }> = ({
  courseId,
}) => {
  const { t } = useTranslation();
  const [batches, setBatches] = useState<GraphBatchView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphHistory(courseId)
        .then((view) => {
          if (!cancelled) setBatches(view.batches);
        })
        .catch((loadError) => {
          if (!cancelled) setError(loadError instanceof Error ? loadError.message : String(loadError));
        });
    };
    load();
    // 生长全程都会改批次时间线（生长开始落 pending 行、终结落定状态）——
    // 组件自订阅事件流：课程详情刷新不经过本组件，靠它自己重拉。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id === courseId) load();
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId]);

  if (error) {
    return <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />;
  }
  if (batches === null) {
    return (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  }
  if (batches.length === 0) {
    return <Empty description={t('learning.learningGraphHistoryEmpty')} />;
  }
  return (
    <div className='flex flex-col gap-12px overflow-y-auto pb-16px'>
      {batches.map((batch) => (
        <div
          key={batch.batch_id}
          className='rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'
        >
          <div className='flex flex-wrap items-baseline gap-8px'>
            <Text bold className='text-13px'>
              {t('learning.learningGraphBatchLabel', { seq: batch.seq })}
            </Text>
            {batch.status === 'pending' && (
              <Tag size='small' color='processing' className='!mx-0'>
                {t('learning.learningGraphBatchPending')}
              </Tag>
            )}
            {batch.status === 'failed' && (
              <Tag size='small' color='red' className='!mx-0'>
                {t('learning.learningGraphBatchFailed')}
              </Tag>
            )}
            <Text type='secondary' className='text-11px'>
              {new Date(batch.created_at).toLocaleString()}
            </Text>
            {batch.note.trim() !== '' && (
              <Text type='secondary' className='text-12px'>
                {batch.note}
              </Text>
            )}
          </div>
          <div className='mt-8px flex flex-col'>
            {batch.nodes.map((node) => (
              <div
                key={node.lesson_id}
                className='flex items-center justify-between gap-8px rd-6px px-6px py-6px hover:bg-[var(--color-fill-1)]'
              >
                <span className='min-w-0 truncate text-13px text-[var(--color-text-1)]'>
                  {node.title}
                </span>
                <span className='flex shrink-0 items-center gap-8px text-11px text-t-tertiary'>
                  <span>{t('learning.learningGraphNodeMinutes', { min: node.estimated_minutes })}</span>
                  <Tag
                    size='small'
                    color={lessonStatusTagColors[node.status]}
                    className='!mx-0'
                  >
                    {statusLabel(node.status, t)}
                  </Tag>
                  {node.completed_at ? (
                    <span>{new Date(node.completed_at).toLocaleDateString()}</span>
                  ) : null}
                </span>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
};

/** 概念表：本课程涉及的概念（教/假定 × 档位 × 节点）+ 跨课程来源。 */
const ConceptTable: React.FC<{ courseId: string }> = ({ courseId }) => {
  const { t } = useTranslation();
  const [rows, setRows] = useState<GraphConceptRowView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphConcepts(courseId)
        .then((view) => {
          if (!cancelled) setRows(view);
        })
        .catch((loadError) => {
          if (!cancelled) setError(loadError instanceof Error ? loadError.message : String(loadError));
        });
    };
    load();
    // 生长落库会铸新概念入登记表——概念表自订阅事件流，落库后重拉。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id === courseId) load();
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId]);

  if (error) {
    return <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />;
  }
  if (rows === null) {
    return (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  }
  if (rows.length === 0) {
    return <Empty description={t('learning.learningGraphConceptsEmpty')} />;
  }
  return (
    <Table
      size='small'
      border={{ wrapper: true, cell: true }}
      pagination={false}
      scroll={{ x: true }}
      data={rows}
      columns={[
        {
          title: t('learning.learningGraphConceptCanonical'),
          dataIndex: 'canonical',
          width: 160,
          render: (_: unknown, row: GraphConceptRowView) => (
            <div className='min-w-0'>
              <div className='truncate font-500 text-[var(--color-text-1)]'>{row.canonical}</div>
              {row.aliases.length > 0 && (
                <div className='truncate text-11px text-t-tertiary'>{row.aliases.join('、')}</div>
              )}
            </div>
          ),
        },
        {
          title: t('learning.learningGraphConceptRefs'),
          dataIndex: 'refs',
          render: (_: unknown, row: GraphConceptRowView) => (
            <div className='flex flex-col gap-2px'>
              {row.refs.map((reference) => (
                <div key={`${reference.lesson_id}:${reference.role}`} className='flex items-center gap-6px text-12px'>
                  <Tag
                    size='small'
                    color={reference.role === 'teaches' ? 'green' : 'orange'}
                    className='!mx-0 shrink-0'
                  >
                    {reference.role === 'teaches'
                      ? t('learning.learningGraphRoleTeaches', {
                          tier: t(`learning.learningGraphTier_${reference.tier}`),
                        })
                      : t('learning.learningGraphRoleAssumes', {
                          tier: t(`learning.learningGraphTier_${reference.tier}`),
                        })}
                  </Tag>
                  <span className='min-w-0 truncate text-[var(--color-text-1)]'>{reference.title}</span>
                  <span className='shrink-0 text-11px text-t-tertiary'>
                    {statusLabel(reference.status, t)}
                  </span>
                </div>
              ))}
            </div>
          ),
        },
        {
          title: t('learning.learningGraphConceptOtherCourses'),
          dataIndex: 'other_courses',
          width: 180,
          render: (_: unknown, row: GraphConceptRowView) =>
            row.other_courses.length > 0 ? (
              <span className='text-12px text-t-secondary'>{row.other_courses.join('、')}</span>
            ) : (
              <span className='text-12px text-t-tertiary'>—</span>
            ),
        },
      ]}
    />
  );
};

export default LearningGraphWorkspace;
