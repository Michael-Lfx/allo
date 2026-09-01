/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Alert, Button, Drawer, Empty, Message, Radio, Spin, Tag, Typography } from '@arco-design/web-react';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import { errorMessage } from '../utils';
import type { CourseDetail, GraphNodeView, LessonStatus } from '../types';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import GraphDagView from './GraphDagView';

const { Text, Title, Paragraph } = Typography;

const STATUS_TAG_COLOR: Record<LessonStatus, string> = {
  not_started: 'gray',
  in_progress: 'arcoblue',
  completed: 'green',
  skipped: 'gray',
};

const STATUS_LABEL_KEY: Record<LessonStatus, string> = {
  not_started: 'learning.learningGraphStatusNotStarted',
  in_progress: 'learning.learningGraphStatusInProgress',
  completed: 'learning.learningGraphStatusCompleted',
  skipped: 'learning.learningGraphStatusSkipped',
};

/**
 * 学习图课程工作区（beta）：取代传统课程的大纲导航。
 * 头部是全图进度概览与「下一步推荐」就绪集；下方 DAG / 列表双视图共享
 * 同一份图投影；节点操作（生成内容、跳过）在详情抽屉内完成。
 */
const LearningGraphWorkspace: React.FC<{
  detail: CourseDetail;
  busyId: string | null;
  onBack: () => void;
  onRefresh: () => void;
}> = ({ detail, busyId, onBack, onRefresh }) => {
  const { t } = useTranslation();
  const model = useLearningAutogenModel();
  const graph = detail.graph;
  const [view, setView] = useState<'dag' | 'list'>('dag');
  const [selected, setSelected] = useState<GraphNodeView | null>(null);

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
  const selectedNode = selected ? nodesById.get(selected.lesson_id) ?? selected : null;
  const selectedLocked = selectedNode ? lockedIds.has(selectedNode.lesson_id) : false;
  const isRecommended = useCallback(
    (lessonId: string) => graph?.recommended.includes(lessonId) ?? false,
    [graph]
  );
  // 进度面板的分段：绿=已完成、蓝=进行中、深灰=已跳过、浅蓝=可学习
  // （未锁定且未开始）、浅灰=未解锁， flexGrow 即段宽比例。
  const segments = useMemo(() => {
    const readyToStudy =
      stats.total - stats.completed - stats.inProgress - stats.skipped - lockedIds.size;
    return [
      { key: 'completed', count: stats.completed, color: 'var(--color-success-6)', label: t('learning.learningGraphStatusCompleted') },
      { key: 'inProgress', count: stats.inProgress, color: 'var(--color-primary-6)', label: t('learning.learningGraphStatusInProgress') },
      { key: 'skipped', count: stats.skipped, color: 'var(--color-text-3)', label: t('learning.learningGraphStatusSkipped') },
      { key: 'ready', count: Math.max(0, readyToStudy), color: 'var(--color-primary-3)', label: t('learning.learningGraphUnlocked') },
      { key: 'locked', count: lockedIds.size, color: 'var(--color-fill-3)', label: t('learning.learningGraphLocked') },
    ];
  }, [lockedIds, stats.completed, stats.inProgress, stats.skipped, stats.total, t]);

  const toggleSkip = useCallback(
    async (node: GraphNodeView) => {
      const next: LessonStatus = node.status === 'skipped' ? 'not_started' : 'skipped';
      try {
        await learningApi.updateLessonProgress(node.lesson_id, next);
        Message.success(
          next === 'skipped'
            ? t('learning.learningGraphSkipDone')
            : t('learning.learningGraphUnskipDone')
        );
        setSelected(null);
        onRefresh();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      }
    },
    [onRefresh, t]
  );

  const generateContent = useCallback(
    async (node: GraphNodeView) => {
      try {
        await learningApi.generateLesson(node.lesson_id, {
          provider_id: model.choice?.provider_id,
          model: model.choice?.model,
        });
        Message.success(t('learning.learningGraphGenerateDone', { title: node.title }));
        setSelected(null);
        onRefresh();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      }
    },
    [model.choice, onRefresh, t]
  );

  if (!graph) {
    return <Empty description={t('learning.learningGraphEmpty')} />;
  }

  const nodeActionBusy = busyId !== null;

  return (
    <div className='flex h-full min-h-0 flex-col gap-12px'>
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

      {/* 进度面板：分段进度条（绿=完成/蓝=进行/灰=跳过/浅蓝=可学/浅灰=未解锁）
          + 图例 + 视图切换，替代原先一排无层级的状态 Tag */}
      <div className='rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-14px py-10px'>
        <div className='mb-8px flex flex-wrap items-center justify-between gap-8px'>
          <Text bold className='text-13px'>
            {t('learning.learningGraphProgressTitle')}
          </Text>
          <Radio.Group
            type='button'
            size='mini'
            value={view}
            onChange={(value) => setView(value as 'dag' | 'list')}
          >
            <Radio value='dag'>{t('learning.learningGraphViewDag')}</Radio>
            <Radio value='list'>{t('learning.learningGraphViewList')}</Radio>
          </Radio.Group>
        </div>
        <div className='h-8px overflow-hidden rounded-full bg-[var(--color-fill-2)]'>
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
        <div className='mt-8px flex flex-wrap items-center gap-x-16px gap-y-4px text-12px text-t-secondary'>
          {segments.map((segment) => (
            <span key={segment.key} className='inline-flex items-center gap-4px'>
              <span
                className='h-8px w-8px rounded-full'
                style={{ backgroundColor: segment.color }}
              />
              {segment.label} {segment.count}
            </span>
          ))}
        </div>
      </div>

      {/* 下一步推荐（就绪集 ≤10）：前置全部完成/已跳过的节点 */}
      {graph.recommended.length > 0 && (
        <div className='flex flex-wrap items-center gap-6px'>
          <Text bold className='text-13px'>
            {t('learning.learningGraphRecommended')}
          </Text>
          {graph.recommended.map((lessonId) => {
            const node = nodesById.get(lessonId);
            if (!node) return null;
            return (
              <Tag
                key={lessonId}
                color='gold'
                className='cursor-pointer'
                onClick={() => setSelected(node)}
              >
                ★ {node.title}
              </Tag>
            );
          })}
        </div>
      )}

      {/* DAG / 列表双视图 */}
      <div className='min-h-0 flex-1 overflow-hidden rounded-8px border-1 border-solid border-[var(--color-border-2)]'>
        {view === 'dag' ? (
          <React.Suspense fallback={<div className='flex h-full items-center justify-center'><Spin /></div>}>
            <GraphDagView
              nodes={graph.nodes}
              edges={graph.edges}
              recommended={graph.recommended}
              lockedIds={lockedIds}
              onSelect={(lessonId) => setSelected(nodesById.get(lessonId) ?? null)}
            />
          </React.Suspense>
        ) : (
          <div className='h-full overflow-y-auto p-8px'>
            {graph.nodes.length === 0 ? (
              <Empty description={t('learning.learningGraphEmpty')} />
            ) : (
              <div className='flex flex-col gap-6px'>
                {[...graph.nodes]
                  .sort((a, b) => a.position - b.position)
                  .map((node) => (
                    <div
                      key={node.lesson_id}
                      className='flex cursor-pointer items-center gap-10px rounded-6px border-1 border-solid border-[var(--color-border-2)] px-12px py-8px hover:bg-[var(--color-fill-1)]'
                      onClick={() => setSelected(node)}
                    >
                      <Tag size='small' color={STATUS_TAG_COLOR[node.status]}>
                        {t(STATUS_LABEL_KEY[node.status])}
                      </Tag>
                      {lockedIds.has(node.lesson_id) && (
                        <Tag size='small' color='gray' className='!mx-0'>
                          {t('learning.learningGraphLocked')}
                        </Tag>
                      )}
                      <span className='flex-1 truncate text-13px'>
                        {isRecommended(node.lesson_id) && (
                          <span className='mr-4px text-[var(--color-warning-6)]'>★</span>
                        )}
                        <span className={lockedIds.has(node.lesson_id) ? 'opacity-55' : ''}>
                          {node.title}
                        </span>
                      </span>
                      {!node.generated && (
                        <Tag size='small' color='orange'>
                          {t('learning.learningGraphNotGenerated')}
                        </Tag>
                      )}
                      <span className='shrink-0 text-12px text-t-tertiary'>
                        {t('learning.learningGraphNodeMinutes', { min: node.estimated_minutes })}
                      </span>
                    </div>
                  ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* 节点详情抽屉：结构信息 + 生成内容 + 跳过操作 */}
      <Drawer
        width={420}
        title={selectedNode?.title ?? ''}
        visible={selectedNode !== null}
        onCancel={() => setSelected(null)}
        footer={null}
        unmountOnExit
      >
        {selectedNode && (
          <div className='flex flex-col gap-12px'>
            <div className='flex flex-wrap items-center gap-6px'>
              <Tag color={STATUS_TAG_COLOR[selectedNode.status]}>
                {t(STATUS_LABEL_KEY[selectedNode.status])}
              </Tag>
              {selectedLocked && (
                <Tag color='gray'>{t('learning.learningGraphLocked')}</Tag>
              )}
              {isRecommended(selectedNode.lesson_id) && (
                <Tag color='gold'>{t('learning.learningGraphRecommendedShort')}</Tag>
              )}
              {selectedNode.generated ? (
                <Tag color='green'>{t('learning.learningGraphGenerated')}</Tag>
              ) : (
                <Tag color='orange'>{t('learning.learningGraphNotGenerated')}</Tag>
              )}
            </div>
            {selectedLocked && (
              <Alert type='info' content={t('learning.learningGraphLockedHint')} />
            )}
            {selectedNode.purpose.trim() !== '' && (
              <div>
                <Text bold>{t('learning.learningGraphNodePurpose')}</Text>
                <Paragraph className='!mb-0'>{selectedNode.purpose}</Paragraph>
              </div>
            )}
            <Text type='secondary'>
              {t('learning.learningGraphNodeMeta', {
                depth: selectedNode.depth + 1,
                min: selectedNode.estimated_minutes,
              })}
            </Text>
            {selectedNode.summary.trim() !== '' && (
              <div>
                <Text bold>{t('learning.learningGraphNodeSummary')}</Text>
                <Paragraph className='!mb-0 max-h-320px overflow-y-auto whitespace-pre-wrap'>
                  {selectedNode.summary}
                </Paragraph>
              </div>
            )}
            <div className='flex flex-wrap gap-8px'>
              {!selectedNode.generated && (
                <Button
                  type='primary'
                  disabled={nodeActionBusy || selectedLocked}
                  onClick={() => void generateContent(selectedNode)}
                >
                  {t('learning.learningGraphGenerateContent')}
                </Button>
              )}
              <Button
                disabled={nodeActionBusy}
                onClick={() => void toggleSkip(selectedNode)}
              >
                {selectedNode.status === 'skipped'
                  ? t('learning.learningGraphUnskip')
                  : t('learning.learningGraphSkip')}
              </Button>
            </div>
          </div>
        )}
      </Drawer>
    </div>
  );
};

export default LearningGraphWorkspace;
