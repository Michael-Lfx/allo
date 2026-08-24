/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Graph, layout as dagreLayout } from '@dagrejs/dagre';
import { Delete } from '@icon-park/react';
import {
  Background,
  BackgroundVariant,
  Controls,
  MarkerType,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Button, Empty, Input, Message, Popconfirm, Spin, Typography } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import { errorMessage } from '../utils';
import type { ConceptGraphSummary, ConceptGraphView } from '../types';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';

const { Text } = Typography;

const NODE_WIDTH = 176;
const NODE_HEIGHT = 64;

type ConceptFlowNode = Node<{ title: string }, 'concept'>;

/** Ellipse-styled atomic-concept node, mirroring the reference rendering. */
const ConceptNode: React.FC<NodeProps<ConceptFlowNode>> = ({ data }) => (
  <div className='flex h-full w-full items-center justify-center overflow-hidden rd-9999px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-14px py-6px text-center text-12px leading-16px text-[var(--color-text-1)]'>
    <span className='line-clamp-3'>{data.title}</span>
  </div>
);

const NODE_TYPES = { concept: ConceptNode };

/**
 * Hierarchical DAG layout via dagre: prerequisites sink to the bottom and the
 * goal rises to the top (rankdir BT), like the reference graph.
 */
function layoutConceptGraph(graph: ConceptGraphView): { nodes: ConceptFlowNode[]; edges: Edge[] } {
  const g = new Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: 'BT', nodesep: 26, ranksep: 64, marginx: 32, marginy: 32 });
  for (const node of graph.nodes) g.setNode(node.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  for (const edge of graph.edges) g.setEdge(edge.from, edge.to);
  dagreLayout(g);

  const nodes = graph.nodes.map<ConceptFlowNode>((node) => {
    const positioned = g.node(node.id);
    return {
      id: node.id,
      type: 'concept',
      position: {
        x: (positioned?.x ?? 0) - NODE_WIDTH / 2,
        y: (positioned?.y ?? 0) - NODE_HEIGHT / 2,
      },
      data: { title: node.title },
    };
  });
  const edges = graph.edges.map<Edge>((edge) => ({
    id: `${edge.from}->${edge.to}`,
    source: edge.from,
    target: edge.to,
    style: { stroke: 'var(--color-text-4)', strokeWidth: 1 },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      width: 12,
      height: 12,
      color: 'var(--color-text-4)',
    },
  }));
  return { nodes, edges };
}

/**
 * Experimental learning feature: decompose a broad learning goal into atomic
 * concepts and render the prerequisite DAG. Generation is one backend model
 * call (1-2 minutes); results persist as rough JSON files server-side.
 */
const ConceptGraphPanel: React.FC = () => {
  const { t } = useTranslation();
  const model = useLearningAutogenModel();
  const [topic, setTopic] = useState('');
  const [summaries, setSummaries] = useState<ConceptGraphSummary[]>([]);
  const [selected, setSelected] = useState<ConceptGraphView | null>(null);
  const [generating, setGenerating] = useState(false);
  const [loadingList, setLoadingList] = useState(true);
  const [loadingGraph, setLoadingGraph] = useState(false);

  const refreshList = useCallback(async () => {
    try {
      setSummaries(await learningApi.listConceptGraphs());
    } catch {
      setSummaries([]);
    } finally {
      setLoadingList(false);
    }
  }, []);

  useEffect(() => {
    void refreshList();
  }, [refreshList]);

  const generate = useCallback(async () => {
    const trimmed = topic.trim();
    if (!trimmed) {
      Message.warning(t('learning.conceptGraphTopicRequired'));
      return;
    }
    setGenerating(true);
    try {
      const record = await learningApi.generateConceptGraph({
        topic: trimmed,
        provider_id: model.choice?.provider_id,
        model: model.choice?.model,
      });
      Message.success(t('learning.conceptGraphGenerated'));
      setSelected(record);
      await refreshList();
    } catch (actionError) {
      Message.error(`${t('learning.conceptGraphFailed')}: ${errorMessage(t, actionError)}`);
    } finally {
      setGenerating(false);
    }
  }, [model.choice, refreshList, t, topic]);

  const openGraph = useCallback(
    async (id: string) => {
      setLoadingGraph(true);
      try {
        setSelected(await learningApi.getConceptGraph(id));
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      } finally {
        setLoadingGraph(false);
      }
    },
    [t]
  );

  const removeGraph = useCallback(
    async (id: string) => {
      try {
        await learningApi.deleteConceptGraph(id);
        setSelected((current) => (current?.id === id ? null : current));
        await refreshList();
      } catch (actionError) {
        Message.error(errorMessage(t, actionError));
      }
    },
    [refreshList, t]
  );

  const { nodes, edges } = useMemo(
    () => (selected ? layoutConceptGraph(selected) : { nodes: [], edges: [] }),
    [selected]
  );

  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Input
          className='!w-360px'
          value={topic}
          onChange={setTopic}
          placeholder={t('learning.conceptGraphPlaceholder')}
          onPressEnter={() => void generate()}
          disabled={generating}
        />
        <LearningModelSelector
          choice={model.choice}
          onChange={(choice) => void model.setChoice(choice)}
          size='small'
          disabled={generating}
        />
        <Button type='primary' loading={generating} onClick={() => void generate()}>
          {t('learning.conceptGraphGenerate')}
        </Button>
        {generating && (
          <Text type='secondary' className='text-12px'>
            {t('learning.conceptGraphGenerating')}
          </Text>
        )}
      </div>

      <div className='flex gap-12px' style={{ height: 640 }}>
        <div className='flex w-240px shrink-0 flex-col gap-4px overflow-y-auto rd-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] p-8px'>
          <Text bold className='px-4px text-12px'>
            {t('learning.conceptGraphHistory')}
          </Text>
          {loadingList ? (
            <div className='flex justify-center py-16px'>
              <Spin />
            </div>
          ) : summaries.length === 0 ? (
            <Empty description={t('learning.conceptGraphHistoryEmpty')} className='!my-16px' />
          ) : (
            summaries.map((summary) => (
              <div
                key={summary.id}
                role='button'
                tabIndex={0}
                className={`flex cursor-pointer items-center justify-between gap-6px rd-6px px-8px py-6px ${
                  selected?.id === summary.id
                    ? 'bg-[var(--color-primary-light-1)]'
                    : 'hover:bg-[var(--color-fill-2)]'
                }`}
                onClick={() => void openGraph(summary.id)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') void openGraph(summary.id);
                }}
              >
                <div className='min-w-0'>
                  <div className='truncate text-12px'>{summary.topic}</div>
                  <div className='text-11px text-t-tertiary'>
                    {t('learning.conceptGraphStats', {
                      nodes: summary.node_count,
                      edges: summary.edge_count,
                    })}
                  </div>
                </div>
                <Popconfirm
                  focusLock
                  title={t('learning.conceptGraphDeleteConfirm')}
                  onOk={() => void removeGraph(summary.id)}
                >
                  <Button
                    size='mini'
                    type='text'
                    icon={<Delete theme='outline' size='12' />}
                    onClick={(event) => event.stopPropagation()}
                  />
                </Popconfirm>
              </div>
            ))
          )}
        </div>

        <div className='relative min-w-0 flex-1 rd-8px border-1 border-solid border-[var(--color-border-2)]'>
          {selected ? (
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={NODE_TYPES}
              fitView
              fitViewOptions={{ padding: 0.12, maxZoom: 1.2 }}
              minZoom={0.05}
              maxZoom={2}
              nodesConnectable={false}
              nodesDraggable={false}
              nodesFocusable={false}
              elementsSelectable={false}
              proOptions={{ hideAttribution: true }}
            >
              <Background
                variant={BackgroundVariant.Dots}
                gap={22}
                size={1.1}
                color='var(--color-text-4)'
              />
              <Controls showInteractive={false} />
            </ReactFlow>
          ) : (
            <div className='flex size-full items-center justify-center'>
              <Empty description={t('learning.conceptGraphEmpty')} />
            </div>
          )}
          {loadingGraph && (
            <div className='absolute inset-0 z-10 flex items-center justify-center bg-[var(--color-bg-1)]/70'>
              <Spin />
            </div>
          )}
        </div>
      </div>

      {selected && (
        <Text type='secondary' className='text-12px'>
          {selected.topic} ·{' '}
          {t('learning.conceptGraphStats', {
            nodes: selected.nodes.length,
            edges: selected.edges.length,
          })}
        </Text>
      )}
    </div>
  );
};

export default ConceptGraphPanel;
