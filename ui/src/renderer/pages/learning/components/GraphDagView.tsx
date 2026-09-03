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
  type Edge,
  type Node,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { GraphEdgeView, GraphNodeView, LessonStatus } from '../types';

const NODE_WIDTH = 124;
const NODE_HEIGHT = 54;

/**
 * 学习进度五态色板（Arco 语义 token，随主题切换）：
 * not_started 中性、in_progress 主色、completed 成功绿、
 * skipped 灰（已声明掌握）、recommended 琥珀（下一步推荐）。
 * 注意：主题体系未导出 warning/primary/success 的 `-6` 色阶变量，
 * 必须带字面 fallback——SVG stroke 的 var() 失效时会 fallback 到
 * 初始值 none，连线直接消失（HTML 属性只是变色，不会消失）。
 */
const STATUS_COLOR: Record<LessonStatus, string> = {
  not_started: 'var(--color-text-4)',
  in_progress: 'var(--color-primary-6, #165dff)',
  completed: 'var(--color-success-6, #00b42a)',
  skipped: 'var(--color-text-3)',
};

type GraphFlowNode = Node<
  {
    title: string;
    minutes: number;
    depth: number;
    status: LessonStatus;
    recommended: boolean;
    generated: boolean;
    locked: boolean;
  },
  'graphNode'
>;

/** 完整节点：左侧状态色条 + 标题 + 层级/估时元信息行；推荐节点琥珀描边、
 * 未解锁节点半透明虚线框。 */
const GraphNodeInner: React.FC<NodeProps<GraphFlowNode>> = ({ data }) => {
  const { t } = useTranslation();
  const accent = data.recommended
    ? 'var(--color-warning-6, #ff7d00)'
    : data.locked
      ? 'var(--color-text-4)'
      : STATUS_COLOR[data.status];
  const border = data.recommended
    ? 'border-warning-6'
    : data.locked
      ? 'border-dashed border-[var(--color-border-2)]'
      : 'border-[var(--color-border-2)]';
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
      title={data.locked ? t('learning.learningGraphLockedHint') : data.title}
      className={`flex h-full w-full items-stretch overflow-hidden rounded-6px border-1 border-solid ${border} ${bg} ${data.locked ? 'opacity-55' : ''}`}
    >
      <span className='w-3px shrink-0' style={{ backgroundColor: accent }} />
      <div className='flex min-w-0 flex-1 flex-col justify-center gap-1px px-6px py-3px text-left'>
        <span className='line-clamp-2 text-12px leading-15px font-500 text-[var(--color-text-1)]'>
          {data.title}
        </span>
        <span className='flex shrink-0 items-center gap-4px text-10px leading-12px text-t-tertiary'>
          <span className='font-500 text-t-secondary'>L{data.depth + 1}</span>
          {data.recommended && <span className='text-[var(--color-warning-6)]'>★</span>}
          <span>{t('learning.learningGraphNodeMinutes', { min: data.minutes })}</span>
        </span>
      </div>
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

const NODE_TYPES = { graphNode: GraphNodeInner };

/** dagre BT 分层布局：前置沉底、目标升至顶层；大图收紧间距控制画布尺寸。
 * 节点对象必须显式携带 width/height：wrapper 尺寸依赖它，缺失时节点
 * 0×0 不可见、边端点错位、MiniMap 无矩形可画。 */
function layoutGraph(
  nodes: GraphNodeView[],
  edges: GraphEdgeView[],
  lockedIds: Set<string>,
  recommendedSet: Set<string>
) {
  const g = new Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: 'BT', nodesep: 16, ranksep: 40, marginx: 24, marginy: 24 });
  for (const node of nodes) g.setNode(node.lesson_id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  for (const edge of edges) g.setEdge(edge.from, edge.to);
  dagreLayout(g);

  const flowNodes = nodes.map<GraphFlowNode>((node) => {
    const positioned = g.node(node.lesson_id);
    return {
      id: node.lesson_id,
      type: 'graphNode',
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      position: {
        x: (positioned?.x ?? 0) - NODE_WIDTH / 2,
        y: (positioned?.y ?? 0) - NODE_HEIGHT / 2,
      },
      data: {
        title: node.title,
        minutes: node.estimated_minutes,
        depth: node.depth,
        status: node.status,
        recommended: false,
        generated: node.generated,
        locked: lockedIds.has(node.lesson_id),
      },
    };
  });
  // 汇入推荐节点的边用琥珀描边做视线引导，其余保持极淡的背景级灰。
  // 琥珀 token 必须带字面 fallback（见 STATUS_COLOR 注释）：var() 失效时
  // SVG stroke 变 none，推荐节点的全部前置线会从图上消失。
  const flowEdges = edges.map<Edge>((edge) => {
    const highlighted = recommendedSet.has(edge.to);
    const stroke = highlighted
      ? 'var(--color-warning-6, #ff7d00)'
      : 'var(--color-border-2)';
    return {
      id: `${edge.from}->${edge.to}`,
      source: edge.from,
      target: edge.to,
      sourceHandle: 'graph-source',
      targetHandle: 'graph-target',
      // non-scaling-stroke：大图 fitView 后 zoom 极低，1px 线会被变换成
      // 亚像素而视觉消失；锁定屏幕像素宽度让连线在任何缩放级别可见。
      style: {
        stroke,
        strokeWidth: highlighted ? 1.5 : 1,
        vectorEffect: 'non-scaling-stroke',
      },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        width: 10,
        height: 10,
        color: stroke,
      },
    };
  });
  return { flowNodes, flowEdges };
}

interface GraphDagViewProps {
  nodes: GraphNodeView[];
  edges: GraphEdgeView[];
  /** 下一步推荐节点（琥珀星标高亮） */
  recommended: string[];
  /** 未解锁节点：存在任一前置未完成/未跳过（半透明虚线示意） */
  lockedIds: Set<string>;
  onSelect: (lessonId: string) => void;
}

/**
 * 学习图 DAG 视图。性能与结构完整性的折衷：
 * 1. dagre 布局仅在输入变化时计算（useMemo）；
 * 2. ≤300 节点全量渲染——`onlyRenderVisibleElements` 会把「一端在视口外」
 *    的整条边裁掉，平移浏览时结构断裂，中小图直接关掉；
 * 3. >300 节点开启视口裁剪保住 500 节点级的流畅度。
 * 画布 key 绑定节点数：图结构变化（重建/增删节点）时重挂载并重新 fitView。
 */
const VIEWPORT_CULL_THRESHOLD = 300;

const GraphDagView: React.FC<GraphDagViewProps> = ({
  nodes,
  edges,
  recommended,
  lockedIds,
  onSelect,
}) => {
  const recommendedSet = useMemo(() => new Set(recommended), [recommended]);
  const { flowNodes, flowEdges } = useMemo(
    () => layoutGraph(nodes, edges, lockedIds, recommendedSet),
    [nodes, edges, lockedIds, recommendedSet]
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
        key={flowNodes.length}
        nodes={markedNodes}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        onlyRenderVisibleElements={flowNodes.length > VIEWPORT_CULL_THRESHOLD}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        minZoom={0.08}
        maxZoom={1.6}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        onNodeClick={(_, node) => onSelect(node.id)}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={24} size={1.2} color='var(--color-fill-2)' />
        <Controls showInteractive={false} />
        <MiniMap
          pannable
          zoomable
          nodeStrokeWidth={2}
          maskColor='var(--color-mask-bg)'
          className='!rounded-6px'
          nodeColor={(node) => {
            const data = (node as GraphFlowNode).data;
            if (data.locked) return 'var(--color-text-4)';
            return data.recommended
              ? 'var(--color-warning-6, #ff7d00)'
              : STATUS_COLOR[data.status];
          }}
        />
      </ReactFlow>
    </div>
  );
};

export default GraphDagView;
