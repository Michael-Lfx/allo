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
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Button, Empty, Input, Message, Popconfirm, Spin, Tag, Typography } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import { errorMessage } from '../utils';
import type {
  AuditSeverity,
  ConceptGraphEdge,
  ConceptGraphFinding,
  ConceptGraphNode,
  ConceptGraphSummary,
  ConceptGraphView,
} from '../types';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';

const { Text } = Typography;

const NODE_WIDTH = 176;
const NODE_HEIGHT = 64;

/** 单个学习单元的分钟预算硬上限（与后端 audit 的 UNIT_MINUTE_CAP 一致）。 */
const UNIT_MINUTE_CAP = 25;

type ConceptFlowNode = Node<{ title: string; min?: number; isAnchor: boolean }, 'concept'>;

/**
 * Ellipse-styled learning-unit node, mirroring the reference rendering.
 * The minute budget renders under the title; a unit over the 25-minute cap
 * is tinted with the danger palette so the workload violation reads at a
 * glance. Handles are required by React Flow for edges to exist at all;
 * with the BT layout the source (toward dependents) sits on top and the
 * target (from prerequisites) on the bottom. They are visually hidden —
 * this graph is read-only and the connecting dots would only add noise.
 */
const ConceptNode: React.FC<NodeProps<ConceptFlowNode>> = ({ data }) => {
  const { t } = useTranslation();
  const overBudget = (data.min ?? 0) > UNIT_MINUTE_CAP;
  const shape = overBudget
    ? 'rd-9999px border-danger-4 bg-danger-light-1'
    : 'rd-9999px border-[var(--color-border-2)] bg-[var(--color-bg-2)]';
  return (
    <div
      className={`flex h-full w-full flex-col items-center justify-center gap-2px overflow-hidden border-1 border-solid px-14px py-6px text-center text-12px leading-16px text-[var(--color-text-1)] ${shape}`}
    >
      <span className='line-clamp-2'>
        {data.isAnchor && (
          <span className='mr-2px shrink-0 text-[var(--color-primary-6)]'>
            {t('learning.conceptGraphAnchorMark')}
          </span>
        )}
        {data.title}
      </span>
      {data.min !== undefined && (
        <span
          className={`shrink-0 text-10px leading-12px ${overBudget ? 'text-danger-6' : 'text-t-tertiary'}`}
        >
          {t('learning.conceptGraphNodeMinutes', { min: data.min })}
        </span>
      )}
      <Handle
        type='target'
        id='concept-target'
        position={Position.Bottom}
        isConnectable={false}
        style={{ opacity: 0, pointerEvents: 'none' }}
      />
      <Handle
        type='source'
        id='concept-source'
        position={Position.Top}
        isConnectable={false}
        style={{ opacity: 0, pointerEvents: 'none' }}
      />
    </div>
  );
};

const NODE_TYPES = { concept: ConceptNode };

/**
 * Hierarchical DAG layout via dagre: prerequisites sink to the bottom and the
 * goal rises to the top (rankdir BT), like the reference graph.
 */
function layoutConceptGraph(graph: {
  nodes: ConceptGraphNode[];
  edges: ConceptGraphEdge[];
}): { nodes: ConceptFlowNode[]; edges: Edge[] } {
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
      data: {
        title: node.title,
        min: node.min,
        isAnchor: node.is_anchor ?? false,
      },
    };
  });
  const edges = graph.edges.map<Edge>((edge) => ({
    id: `${edge.from}->${edge.to}`,
    source: edge.from,
    target: edge.to,
    // Explicit handle ids silence React Flow's error#008 and pin the
    // connection to the hidden source/target handles above.
    sourceHandle: 'concept-source',
    targetHandle: 'concept-target',
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
 * Experimental learning feature: decompose a broad learning goal into a
 * network of learning units (each a 25-minute study session with a minute
 * budget) and render the dependency DAG. Generation is one full backend
 * model call plus light audit-gate repair rounds; results persist as rough
 * JSON files server-side.
 */
const ConceptGraphPanel: React.FC = () => {
  const { t } = useTranslation();
  const model = useLearningAutogenModel();
  const [topic, setTopic] = useState('');
  const [summaries, setSummaries] = useState<ConceptGraphSummary[]>([]);
  const [selected, setSelected] = useState<ConceptGraphView | null>(null);
  const [generating, setGenerating] = useState(false);
  const [repairing, setRepairing] = useState(false);
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

  const repair = useCallback(async () => {
    if (!selected) return;
    setRepairing(true);
    try {
      const updated = await learningApi.repairConceptGraph(selected.id, {});
      Message.success(t('learning.conceptGraphRepairSuccess'));
      setSelected(updated);
      await refreshList();
    } catch (actionError) {
      Message.error(`${t('learning.conceptGraphRepairFailed')}: ${errorMessage(t, actionError)}`);
    } finally {
      setRepairing(false);
    }
  }, [refreshList, selected, t]);

  // The whole graph renders as-is: every node is a learning unit, there is
  // no milestone/atom split anymore.
  const { nodes, edges } = useMemo(
    () => (selected ? layoutConceptGraph(selected) : { nodes: [], edges: [] }),
    [selected]
  );

  // Total promised workload: the sum of every unit's minute budget. Units
  // without a budget are skipped, so the figure is a lower bound at worst.
  const totalMinutes = useMemo(
    () => selected?.nodes.reduce((sum, node) => sum + (node.min ?? 0), 0) ?? 0,
    [selected]
  );
  const workloadLabel = useMemo(() => {
    if (totalMinutes === 0) return null;
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return hours > 0
      ? t('learning.conceptGraphWorkloadHours', { hours, minutes })
      : t('learning.conceptGraphWorkloadMinutes', { minutes });
  }, [t, totalMinutes]);

  // Findings sorted by severity (danger → warning → info) so the worst
  // issues read first in the audit panel.
  const orderedFindings = useMemo<ConceptGraphFinding[]>(() => {
    if (!selected) return [];
    const weight: Record<AuditSeverity, number> = { danger: 0, warning: 1, info: 2 };
    return [...selected.audit.findings].sort(
      (a, b) => weight[a.severity] - weight[b.severity]
    );
  }, [selected]);

  const severityLabel = (severity: AuditSeverity) =>
    severity === 'danger'
      ? t('learning.conceptGraphSeverityDanger')
      : severity === 'warning'
        ? t('learning.conceptGraphSeverityWarning')
        : t('learning.conceptGraphSeverityInfo');
  const severityColor: Record<AuditSeverity, string> = {
    danger: 'red',
    warning: 'orange',
    info: 'blue',
  };
  const nodeTitle = (id: string) =>
    selected?.nodes.find((node) => node.id === id)?.title ?? id;

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
        <div className='flex flex-col gap-8px rd-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] p-12px'>
          <div className='flex items-center justify-between gap-8px'>
            <div className='flex min-w-0 items-center gap-8px'>
              <Text bold className='text-12px'>
                {t('learning.conceptGraphAudit')}
              </Text>
              <Text type='secondary' className='truncate text-12px'>
                {selected.topic} ·{' '}
                {t('learning.conceptGraphStats', {
                  nodes: selected.nodes.length,
                  edges: selected.edges.length,
                })}
                {workloadLabel && (
                  <>
                    {' · '}
                    <Text className='text-12px'>{workloadLabel}</Text>
                  </>
                )}
              </Text>
            </div>
            <Button
              size='small'
              type='primary'
              status='warning'
              loading={repairing}
              disabled={orderedFindings.length === 0}
              onClick={() => void repair()}
            >
              {t('learning.conceptGraphRepair')}
            </Button>
          </div>
          <Text type='secondary' className='text-12px'>
            {t('learning.conceptGraphRefDrop', {
              count: selected.audit.ref_drop_count,
              rate: (selected.audit.ref_drop_rate * 100).toFixed(1),
            })}
          </Text>
          {orderedFindings.length === 0 ? (
            <Empty description={t('learning.conceptGraphAuditEmpty')} className='!my-4px' />
          ) : (
            orderedFindings.map((finding, index) => (
              <div
                key={`${finding.kind}-${index}`}
                className='flex flex-col gap-4px rd-6px bg-[var(--color-fill-1)] px-8px py-6px'
              >
                <div className='flex items-center gap-6px'>
                  <Tag color={severityColor[finding.severity]} size='small'>
                    {severityLabel(finding.severity)}
                  </Tag>
                  <Text className='truncate text-12px'>{finding.kind}</Text>
                </div>
                <Text type='secondary' className='text-12px'>
                  {finding.message}
                </Text>
                {finding.node_ids.length > 0 && (
                  <div className='flex flex-wrap gap-4px'>
                    {finding.node_ids.map((id) => (
                      <Tag key={id} size='small' className='!text-11px'>
                        {nodeTitle(id)}
                      </Tag>
                    ))}
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
};

export default ConceptGraphPanel;
