/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Alert, Button, Empty, Modal, Spin, Tag, Typography } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { statusColors } from '../constants';
import { statusLabel } from '../utils';
import type {
  Activity,
  AttemptResult,
  CourseDetail,
  GraphNodeView,
  Lesson,
  LessonStatus,
} from '../types';
import { LessonBlock } from './CourseWorkspace';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import GraphDagView from './GraphDagView';

const { Text, Title, Paragraph } = Typography;

/**
 * 学习图课程工作区（beta）：以「节点课时学习」为主体——左栏是继续学习
 * 就绪集导航，中央内容区复用普通课程的 LessonBlock（Markdown 正文 +
 * 练习题作答 + 完成课时 + 生成进度反馈）。全图 DAG 降级为弹窗入口：
 * 图只在刚生成、零进度时自动弹出一次，之后的日常学习都在内容区完成。
 */
const LearningGraphWorkspace: React.FC<{
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptResult>;
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
  const graph = detail.graph;

  // 学习图课程只有一个隐含模块；课时的完整视图（含 activities）从这里取，
  // graph.nodes 只承载结构投影（标题/状态/深度/生成标记）。
  const lessons = useMemo(
    () => detail.modules.flatMap((module) => module.lessons),
    [detail.modules]
  );
  const lessonsById = useMemo(
    () => new Map(lessons.map((lesson) => [lesson.id, lesson])),
    [lessons]
  );

  const [selectedLessonId, setSelectedLessonId] = useState<string | null>(
    () => graph?.recommended[0] ?? null
  );
  const [graphOpen, setGraphOpen] = useState(false);
  // 全图自动弹出只发生一次；用户关闭后（或已有进度后）不再打扰。
  const autoOpenedRef = useRef(false);

  const stats = useMemo(() => {
    const nodes = graph?.nodes ?? [];
    const count = (status: LessonStatus) => nodes.filter((node) => node.status === status).length;
    return {
      total: nodes.length,
      completed: count('completed'),
      inProgress: count('in_progress'),
      skipped: count('skipped'),
      notStarted: count('not_started'),
      generated: nodes.filter((node) => node.generated).length,
      minutes: nodes.reduce((sum, node) => sum + node.estimated_minutes, 0),
    };
  }, [graph]);

  // 零进度（刚生成的课程）：首次进入自动弹出全图，先纵览再学习。
  useEffect(() => {
    if (autoOpenedRef.current) return;
    if (stats.total > 0 && stats.completed + stats.inProgress + stats.skipped === 0) {
      autoOpenedRef.current = true;
      setGraphOpen(true);
    }
  }, [stats.total, stats.completed, stats.inProgress, stats.skipped]);

  const nodesById = useMemo(
    () => new Map((graph?.nodes ?? []).map((node) => [node.lesson_id, node])),
    [graph]
  );
  // 解锁 = 不存在任何「未完成且未跳过」的前置（根节点天然解锁）。
  // 前端从边表推导：完成/跳过集合 ∩ 入边 from，命中即锁定 to。
  const lockedIds = useMemo(() => {
    const locked = new Set<string>();
    if (!graph) return locked;
    const satisfied = new Set(
      graph.nodes
        .filter((node) => node.status === 'completed' || node.status === 'skipped')
        .map((node) => node.lesson_id)
    );
    for (const edge of graph.edges) {
      if (!satisfied.has(edge.from)) locked.add(edge.to);
    }
    return locked;
  }, [graph]);
  const isRecommended = useCallback(
    (lessonId: string) => graph?.recommended.includes(lessonId) ?? false,
    [graph]
  );
  const primaryReady = useMemo(() => {
    const first = graph?.recommended[0];
    return first ? nodesById.get(first) ?? null : null;
  }, [graph, nodesById]);
  const otherReady = useMemo(
    () =>
      (graph?.recommended.slice(1) ?? [])
        .map((id) => nodesById.get(id))
        .filter((node): node is GraphNodeView => Boolean(node)),
    [graph, nodesById]
  );
  // 进度面板的分段：绿=已完成、蓝=进行中、深灰=已跳过、浅蓝=可学习
  // （未锁定且未开始）、浅灰=未解锁， flexGrow 即段宽比例。
  const segments = useMemo(() => {
    const readyToStudy =
      stats.total - stats.completed - stats.inProgress - stats.skipped - lockedIds.size;
    return [
      // primary/success 的 `-6` 色阶变量主题未导出，必须带字面 fallback
      // （同 GraphDagView 的 STATUS_COLOR 注释）。
      { key: 'completed', count: stats.completed, color: 'var(--color-success-6, #00b42a)', label: t('learning.learningGraphStatusCompleted') },
      { key: 'inProgress', count: stats.inProgress, color: 'var(--color-primary-6, #165dff)', label: t('learning.learningGraphStatusInProgress') },
      { key: 'skipped', count: stats.skipped, color: 'var(--color-text-3)', label: t('learning.learningGraphStatusSkipped') },
      { key: 'ready', count: Math.max(0, readyToStudy), color: 'var(--color-primary-3, #94bfff)', label: t('learning.learningGraphUnlocked') },
      { key: 'locked', count: lockedIds.size, color: 'var(--color-fill-3)', label: t('learning.learningGraphLocked') },
    ];
  }, [lockedIds, stats.completed, stats.inProgress, stats.skipped, stats.total, t]);

  // 完成节点后推荐首位会推进：内容区自动跟随到下一个应学节点；
  // 用户手动点选不被覆盖（与普通课程工作区的跟随策略一致）。
  const recommendedFirst = graph?.recommended[0] ?? null;
  const lastRecommendedIdRef = useRef<string | null>(recommendedFirst);
  useEffect(() => {
    if (recommendedFirst && recommendedFirst !== lastRecommendedIdRef.current) {
      lastRecommendedIdRef.current = recommendedFirst;
      setSelectedLessonId(recommendedFirst);
    }
  }, [recommendedFirst]);

  // 选中的课时在前端数据里消失时（课程刷新后结构变化）回退到推荐首位
  const selectedLesson = useMemo(
    () =>
      (selectedLessonId ? lessonsById.get(selectedLessonId) : null) ??
      (recommendedFirst ? lessonsById.get(recommendedFirst) : null) ??
      null,
    [lessonsById, selectedLessonId, recommendedFirst]
  );
  const selectedNode: GraphNodeView | null = selectedLesson
    ? nodesById.get(selectedLesson.id) ?? null
    : null;
  const selectedLocked =
    selectedLesson && selectedNode ? lockedIds.has(selectedLesson.id) : false;
  const primaryLesson = primaryReady ? lessonsById.get(primaryReady.lesson_id) ?? null : null;

  if (!graph) {
    return <Empty description={t('learning.learningGraphEmpty')} />;
  }

  return (
    <div className='app-page-shell h-full w-full box-border overflow-y-auto'>
      <div className='mx-auto flex h-full w-full flex-col gap-10px md:max-w-1400px'>
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
        <LearningModelSelector
          choice={model.choice}
          onChange={(choice) => void model.setChoice(choice)}
          size='small'
        />
      </div>

      {/* 主从布局：左=继续学习就绪集导航，右=选中节点的课时学习区 */}
      <div className='flex min-h-0 flex-1 gap-12px'>
        <aside className='flex w-280px shrink-0 flex-col gap-10px overflow-y-auto'>
          {/* 主推荐卡：当前应学的节点——最重要的学习入口，点击即进入学习 */}
          <div
            className='cursor-pointer rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px transition-colors hover:border-[var(--color-primary-6)]'
            onClick={() => primaryReady && setSelectedLessonId(primaryReady.lesson_id)}
          >
            <div className='mb-6px flex items-center justify-between gap-6px'>
              <Text bold className='text-13px'>
                {t('learning.learningGraphContinueTitle')}
              </Text>
              {primaryReady &&
                (primaryReady.generated ? (
                  <Tag size='small' color='green' className='!mx-0'>
                    {t('learning.learningGraphGenerated')}
                  </Tag>
                ) : (
                  <Tag size='small' color='orange' className='!mx-0'>
                    {t('learning.learningGraphNotGenerated')}
                  </Tag>
                ))}
            </div>
            {primaryReady ? (
              <>
                <div className='text-14px font-600 leading-20px text-[var(--color-text-1)]'>
                  {primaryReady.title}
                </div>
                {primaryReady.purpose.trim() !== '' && (
                  <Paragraph className='!mb-6px !mt-4px line-clamp-3 text-12px leading-18px text-t-secondary'>
                    {primaryReady.purpose}
                  </Paragraph>
                )}
                <div className='flex flex-wrap items-center gap-x-8px gap-y-2px text-11px text-t-tertiary'>
                  <span>L{primaryReady.depth + 1}</span>
                  <span>
                    {t('learning.learningGraphNodeMinutes', { min: primaryReady.estimated_minutes })}
                  </span>
                  <span>
                    {t('learning.learningGraphNodePrerequisites', {
                      count: primaryReady.prerequisite_count,
                    })}
                  </span>
                </div>
                {primaryLesson && !primaryReady.generated && (
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
          {otherReady.length > 0 && (
            <div className='rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
              <Text type='secondary' className='text-12px'>
                {t('learning.learningGraphReadyMore', { count: otherReady.length })}
              </Text>
              <div className='mt-4px flex flex-col'>
                {otherReady.map((node) => (
                  <button
                    key={node.lesson_id}
                    type='button'
                    onClick={() => setSelectedLessonId(node.lesson_id)}
                    className={`rd-6px flex cursor-pointer items-center justify-between gap-8px border-none bg-transparent px-6px py-6px text-left font-inherit text-13px transition-colors hover:bg-[var(--color-fill-1)] ${
                      selectedLessonId === node.lesson_id
                        ? 'bg-primary-1 font-500 text-primary-6'
                        : 'text-[var(--color-text-1)]'
                    }`}
                  >
                    <span className='min-w-0 truncate'>{node.title}</span>
                    <span className='shrink-0 text-11px text-t-tertiary'>
                      {t('learning.learningGraphNodeMinutes', { min: node.estimated_minutes })}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {/* 进度面板 */}
          <div className='mt-auto rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
            <Text bold className='text-12px'>
              {t('learning.learningGraphProgressTitle')}
            </Text>
            <div className='mt-6px h-6px overflow-hidden rounded-full bg-[var(--color-fill-2)]'>
              {stats.total === 0 ? null : (
                <div className='flex h-full'>
                  {segments
                    .filter((segment) => segment.count > 0)
                    .map((segment) => (
                      <span
                        key={segment.key}
                        className='h-full'
                        style={{ flexGrow: segment.count, backgroundColor: segment.color }}
                      />
                    ))}
                </div>
              )}
            </div>
            <div className='mt-6px flex flex-wrap items-center gap-x-12px gap-y-2px text-11px text-t-secondary'>
              {segments.map((segment) => (
                <span key={segment.key} className='inline-flex items-center gap-4px'>
                  <span
                    className='h-7px w-7px rounded-full'
                    style={{ backgroundColor: segment.color }}
                  />
                  {segment.label} {segment.count}
                </span>
              ))}
            </div>
          </div>
          {/* 全图入口：DAG 降级为弹窗——图只在首次纵览与宏观导航时需要 */}
          <Button long onClick={() => setGraphOpen(true)}>
            {t('learning.learningGraphOpenGraph')}
          </Button>
        </aside>

        {/* 中央内容区：选中节点的完整课时学习（正文/练习题/完成/追加练习） */}
        <section className='flex min-w-0 flex-1 flex-col overflow-hidden rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
          {!selectedLesson || !selectedNode ? (
            <div className='flex h-full flex-1 items-center justify-center'>
              <Empty description={t('learning.learningGraphContentEmpty')} />
            </div>
          ) : (
            <div className='h-full overflow-y-auto p-16px'>
              <div className='flex flex-wrap items-center gap-8px'>
                <Title heading={4} className='!m-0'>
                  {selectedLesson.title}
                </Title>
                <Tag size='small' color={statusColors[selectedLesson.status]} className='!mx-0'>
                  {statusLabel(selectedLesson.status, t)}
                </Tag>
                {selectedLocked && (
                  <Tag size='small' color='gray' className='!mx-0'>
                    {t('learning.learningGraphLocked')}
                  </Tag>
                )}
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
              {selectedLocked ? (
                <>
                  <Alert type='info' content={t('learning.learningGraphLockedHint')} className='!mt-10px' />
                  {selectedNode.purpose.trim() !== '' && (
                    <Paragraph className='!mb-0 !mt-10px text-t-secondary'>
                      {selectedNode.purpose}
                    </Paragraph>
                  )}
                  <Text type='secondary' className='mt-10px block'>
                    {t('learning.learningGraphNodeMeta', {
                      depth: selectedNode.depth + 1,
                      min: selectedNode.estimated_minutes,
                    })}
                  </Text>
                </>
              ) : (
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
              )}
            </div>
          )}
        </section>
      </div>
      </div>

      {/* 全图弹窗：DAG 视图（可从图上点任意节点跳转学习，含已完成复习） */}
      <Modal
        title={t('learning.learningGraphTabTitle')}
        visible={graphOpen}
        footer={null}
        onCancel={() => setGraphOpen(false)}
        style={{ width: 1200 }}
        unmountOnExit
      >
        <div className='h-[70vh]'>
          <React.Suspense
            fallback={
              <div className='flex h-full items-center justify-center'>
                <Spin />
              </div>
            }
          >
            <GraphDagView
              nodes={graph.nodes}
              edges={graph.edges}
              recommended={graph.recommended}
              lockedIds={lockedIds}
              onSelect={(lessonId) => {
                setGraphOpen(false);
                setSelectedLessonId(lessonId);
              }}
            />
          </React.Suspense>
        </div>
      </Modal>
    </div>
  );
};

export default LearningGraphWorkspace;
