/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Graph, layout as dagreLayout } from '@dagrejs/dagre';
import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  useStore,
  type Edge,
  type Node,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GraphEdgeView, GraphNodeView, LessonStatus } from '../types';

const NODE_WIDTH = 176;
const NODE_HEIGHT = 64;

/** 500 节点级大图的可交互下限：低于该缩放切换为宏观点阵（LOD） */
const LOD_ZOOM = 0.45;

/**
 * 学习进度五态色板（Arco 语义 token，随主题切换）：
 * not_started 中性、in_progress 主色、completed 成功绿、
 * skipped 灰（已声明掌握）、recommended 琥珀（下一步推荐）。
 */
const STATUS_COLOR: Record<LessonStatus, string> = {
  not_started: 'var(--color-text-4)',
  in_progress: 'var(--color-primary-6)',
  completed: 'var(--color-success-6)',
  skipped: 'var(--color-text-3)',
};

type GraphFlowNode = Node<
  { title: string; minutes: number; status: LessonStatus; recommended: boolean; generated: boolean },
  'graphNode'
>;

/** 宏观点阵节点：zoom 低于阈值时整个网络退化为色点阵（学习者仍能读出结构与进度） */
const GraphDot: React.FC<NodeProps<GraphFlowNode>> = ({ data }) => (
  <div
    className='h-full w-full rounded-full'
    style={{
      backgroundColor: data.recommended
        ? 'var(--color-warning-6)'
        : STATUS_COLOR[data.status],
      outline: data.recommended ? '2px solid var(--color-warning-6)' : 'none',
    }}
    title={data.title}
  />
);

/** 完整节点：进度着色 + 推荐星标 + 估时 */
const GraphNodeInner: React.FC<NodeProps<GraphFlowNode>> = ({ data }) => {
  const { t } = useTranslation();
  const border = data.recommended
    ? 'border-warning-6'
    : `border-[var(--color-border-2)]`;
  const bg =
    data.status === 'completed'
      ? 'bg-success-light-1'
      : data.status === 'in_progress'
        ? 'bg-primary-light-1'
        : data.status === 'skipped'
          ? 'bg-[var(--color-fill-1)]'
          : 'bg-[var(--color-bg-2)]';
  return (
    <div
      className={`flex h-full w-full flex-col items-center justify-center gap-2px overflow-hidden rounded-8px border-1 border-solid px-10px py-6px text-center text-12px leading-16px text-[var(--color-text-1)] ${border} ${bg}`}
    >
      <span className='line-clamp-2'>
        {data.recommended && (
          <span className='mr-2px shrink-0 text-[var(--color-warning-6)]'>★</span>
        )}
        {data.title}
      </span>
      <span className='shrink-0 text-10px leading-12px text-t-tertiary'>
        {t('learning.learningGraphNodeMinutes', { min: data.minutes })}
      </span>
      <Handle
        type='target'
        id='graph-target'
        position={Position.Bottom}
        isConnectable={false}
        style={{ opacity: 0, pointerEvents: 'none' }}
      />
      <Handle
        type='source'
        id='graph-source'
        position={Position.Top}
        isConnectable={false}
        style={{ opacity: 0, pointerEvents: 'none' }}
      />
    </div>
  );
};

/**
 * LOD 由节点自身订阅相机缩放实现：只有视口内的节点参与渲染
 * （`onlyRenderVisibleElements`），所以订阅者恒为少量；跨越阈值才切换
 * 形态，避免每帧 setState。
 */
const GraphNodeByZoom: React.FC<NodeProps<GraphFlowNode>> = (props) => {
  const zoom = useStore((state) => state.transform[2]);
  if (zoom < LOD_ZOOM) return <GraphDot {...props} />;
  return <GraphNodeInner {...props} />;
};

const NODE_TYPES = { graphNode: GraphNodeByZoom };

/** dagre BT 分层布局：前置沉底、目标升至顶层；大图收紧间距控制画布尺寸 */
function layoutGraph(nodes: GraphNodeView[], edges: GraphEdgeView[]) {
  const g = new Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: 'BT', nodesep: 22, ranksep: 56, marginx: 32, marginy: 32 });
  for (const node of nodes) g.setNode(node.lesson_id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  for (const edge of edges) g.setEdge(edge.from, edge.to);
  dagreLayout(g);

  const flowNodes = nodes.map<GraphFlowNode>((node) => {
    const positioned = g.node(node.lesson_id);
    return {
      id: node.lesson_id,
      type: 'graphNode',
      position: {
        x: (positioned?.x ?? 0) - NODE_WIDTH / 2,
        y: (positioned?.y ?? 0) - NODE_HEIGHT / 2,
      },
      data: {
        title: node.title,
        minutes: node.estimated_minutes,
        status: node.status,
        recommended: false,
        generated: node.generated,
      },
    };
  });
  const flowEdges = edges.map<Edge>((edge) => ({
    id: `${edge.from}->${edge.to}`,
    source: edge.from,
    target: edge.to,
    sourceHandle: 'graph-source',
    targetHandle: 'graph-target',
    style: { stroke: 'var(--color-text-4)', strokeWidth: 1 },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      width: 12,
      height: 12,
      color: 'var(--color-text-4)',
    },
  }));
  return { flowNodes, flowEdges };
}

interface GraphDagViewProps {
  nodes: GraphNodeView[];
  edges: GraphEdgeView[];
  /** 下一步推荐节点（琥珀星标高亮） */
  recommended: string[];
  onSelect: (lessonId: string) => void;
}

/**
 * 学习图 DAG 视图。500 节点规模的三层性能保障：
 * 1. dagre 布局仅在输入变化时计算（useMemo）；
 * 2. `onlyRenderVisibleElements` 只渲染视口内节点/边；
 * 3. zoom 低于 0.45 时节点退化为色点（宏观点阵），宏观结构视角永远保留。
 */
const GraphDagView: React.FC<GraphDagViewProps> = ({ nodes, edges, recommended, onSelect }) => {
  const recommendedSet = useMemo(() => new Set(recommended), [recommended]);
  const { flowNodes, flowEdges } = useMemo(
    () => layoutGraph(nodes, edges),
    [nodes, edges]
  );
  const markedNodes = useMemo(
    () =>
      flowNodes.map((node) => ({
        ...node,
        data: { ...node.data, recommended: recommendedSet.has(node.id) },
      })),
    [flowNodes, recommendedSet]
  );

  return (
    <div className='h-full w-full'>
      <ReactFlow
        nodes={markedNodes}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        onlyRenderVisibleElements
        fitView
        minZoom={0.08}
        maxZoom={1.6}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        onNodeClick={(_, node) => onSelect(node.id)}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={20} size={1} />
        <Controls showInteractive={false} />
        <MiniMap
          pannable
          zoomable
          nodeStrokeWidth={2}
          maskColor='var(--color-mask-bg)'
          nodeColor={(node) => {
            const data = (node as GraphFlowNode).data;
            return data.recommended ? 'var(--color-warning-6)' : STATUS_COLOR[data.status];
          }}
        />
      </ReactFlow>
    </div>
  );
};

export default GraphDagView;
