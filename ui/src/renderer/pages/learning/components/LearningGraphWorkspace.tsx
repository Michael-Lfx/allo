/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Drawer, Empty, Message, Radio, Spin, Tag, Typography } from '@arco-design/web-react';
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
  const selectedNode = selected ? nodesById.get(selected.lesson_id) ?? selected : null;
  const isRecommended = useCallback(
    (lessonId: string) => graph?.recommended.includes(lessonId) ?? false,
    [graph]
  );

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
      {/* 头部：返回 + 标题 + Beta + 模型选择 + 进度概览 + 视图切换 */}
      <div className='flex flex-wrap items-center justify-between gap-12px'>
        <div className='flex items-center gap-10px'>
          <Button onClick={onBack}>{t('learning.back')}</Button>
          <Title heading={4} className='!m-0'>
            {detail.course.title}
          </Title>
          <Tag size='small' color='orangered' className='!mx-0'>
            {t('learning.learningGraphBeta')}
          </Tag>
        </div>
        <LearningModelSelector
          choice={model.choice}
          onChange={(choice) => void model.setChoice(choice)}
          size='small'
        />
      </div>
      <div className='flex flex-wrap items-center gap-8px text-12px text-t-secondary'>
        <Text type='secondary'>
          {t('learning.learningGraphProgressLine', {
            completed: stats.completed,
            total: stats.total,
            skipped: stats.skipped,
          })}
        </Text>
        <Tag size='small' color='green'>{t('learning.learningGraphStatusCompleted')} {stats.completed}</Tag>
        <Tag size='small' color='arcoblue'>{t('learning.learningGraphStatusInProgress')} {stats.inProgress}</Tag>
        <Tag size='small' color='gray'>{t('learning.learningGraphStatusSkipped')} {stats.skipped}</Tag>
        <Tag size='small' color='gray'>{t('learning.learningGraphGeneratedShort')} {stats.generated}/{stats.total}</Tag>
        <div className='ml-auto'>
          <Radio.Group
            type='button'
            size='small'
            value={view}
            onChange={(value) => setView(value as 'dag' | 'list')}
          >
            <Radio value='dag'>{t('learning.learningGraphViewDag')}</Radio>
            <Radio value='list'>{t('learning.learningGraphViewList')}</Radio>
          </Radio.Group>
        </div>
      </div>

      {graph.goal.trim() !== '' && (
        <Text type='secondary'>
          {t('learning.learningGraphGoal')}: {graph.goal}
        </Text>
      )}

      {/* 下一步推荐（就绪集 ≤10）：前置全部完成/已跳过的节点 */}
      {graph.recommended.length > 0 && (
        <div className='flex flex-wrap items-center gap-6px'>
          <Text bold>{t('learning.learningGraphRecommended')}</Text>
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
                      <span className='flex-1 truncate text-13px'>
                        {isRecommended(node.lesson_id) && (
                          <span className='mr-4px text-[var(--color-warning-6)]'>★</span>
                        )}
                        {node.title}
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
              {isRecommended(selectedNode.lesson_id) && (
                <Tag color='gold'>{t('learning.learningGraphRecommendedShort')}</Tag>
              )}
              {selectedNode.generated ? (
                <Tag color='green'>{t('learning.learningGraphGenerated')}</Tag>
              ) : (
                <Tag color='orange'>{t('learning.learningGraphNotGenerated')}</Tag>
              )}
            </div>
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
                  disabled={nodeActionBusy}
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
