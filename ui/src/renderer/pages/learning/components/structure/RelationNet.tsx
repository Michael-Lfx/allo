import dagre from '@dagrejs/dagre';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BlockView, StructureView, StructureEdgeView } from '../../types';
import { BlockDetail } from './BlockDetail';
import { EDGE_COLORS, HUB_COLOR, buildGroupPalette, edgeEndpoints } from './palette';

const W = 150;
const H = 30;

/**
 * 关系图谱（dagre LR 分层 + 自绘 SVG 连线 + HTML button 节点；xyflow 弃用，
 * v1 不引 echarts）：**边方向反转展示——assumes = 门**（从需求方指向它要求的
 * 前置，视觉上表示「先学前置才轮到本块」）；composes 组装、contrasts 对照。
 * 枢纽 Top10 琥珀描边；按主干隐藏、按关系过滤；未收录端点虚线菱形占位（防御；
 * 契约保证端点解析域 = 块表）。
 */
export function RelationNet({ structure }: { structure: StructureView }) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<BlockView | null>(null);
  const [relationFilter, setRelationFilter] = useState<string>('__all__');
  const [hiddenSpines, setHiddenSpines] = useState<Set<string>>(new Set());
  const colorFor = useMemo(
    () => buildGroupPalette(structure.spines.map((spine) => spine.name)),
    [structure.spines]
  );

  const hubSet = useMemo(
    () => new Set(structure.hubs.map((hub) => hub.name)),
    [structure.hubs]
  );

  // 可见节点：按主干隐藏过滤；边：两端可见 + 关系过滤。
  const nodeByName = useMemo(() => {
    const map = new Map<string, BlockView>();
    for (const block of structure.blocks) map.set(block.name, block);
    return map;
  }, [structure.blocks]);
  const visibleBlocks = useMemo(
    () => structure.blocks.filter((block) => !hiddenSpines.has(block.spine)),
    [structure.blocks, hiddenSpines]
  );
  const visibleNames = useMemo(
    () => new Set(visibleBlocks.map((block) => block.name)),
    [visibleBlocks]
  );
  const visibleEdges = useMemo(
    () =>
      structure.edges.filter(
        (edge) =>
          visibleNames.has(edge.source) &&
          visibleNames.has(edge.target) &&
          (relationFilter === '__all__' || edge.edge_type === relationFilter)
      ),
    [structure.edges, visibleNames, relationFilter]
  );

  // 未收录端点（防御占位）：出现在边上、但不在块表名集里的名字。
  const unregistered = useMemo(() => {
    const names = new Set<string>();
    for (const edge of structure.edges) {
      for (const endpoint of [edge.source, edge.target]) {
        if (!nodeByName.has(endpoint)) names.add(endpoint);
      }
    }
    return [...names];
  }, [structure.edges, nodeByName]);

  const toggleSpine = (spine: string) => {
    setHiddenSpines((current) => {
      const next = new Set(current);
      if (next.has(spine)) next.delete(spine);
      else next.add(spine);
      return next;
    });
  };

  return (
    <div className='flex flex-col gap-8px'>
      <div className='flex flex-wrap items-center gap-8px text-12px'>
        <TextLabel>{t('learningGraphStructRelationFilter')}:</TextLabel>
        {[
          ['__all__', t('learningGraphStructRelationAll')],
          ['assumes', t('learningGraphStructRelation_assumes')],
          ['composes', t('learningGraphStructRelation_composes')],
          ['contrasts', t('learningGraphStructRelation_contrasts')],
        ].map(([value, label]) => (
          <FilterChip
            key={value}
            active={relationFilter === value}
            onClick={() => setRelationFilter(value)}
          >
            {label}
          </FilterChip>
        ))}
        <span className='mx-4px text-t-tertiary'>·</span>
        <TextLabel>{t('learningGraphStructHideSpines')}:</TextLabel>
        {structure.spines.map((spine) => (
          <FilterChip
            key={spine.name}
            active={hiddenSpines.has(spine.name)}
            onClick={() => toggleSpine(spine.name)}
          >
            <span
              className='mr-4px inline-block h-8px w-8px rd-2px'
              style={{ background: colorFor(spine.name) }}
            />
            {spine.name}
          </FilterChip>
        ))}
      </div>
      {structure.hubs.length > 0 && (
        <div className='flex flex-wrap items-center gap-6px text-11px text-t-secondary'>
          {t('learningGraphStructHubs')}:
          {structure.hubs.map((hub) => (
            <span
              key={hub.name}
              className='rounded-4px border-1 border-solid px-4px py-1px'
              style={{ borderColor: HUB_COLOR, color: HUB_COLOR }}
            >
              {hub.name} · {hub.in_degree + hub.out_degree}
            </span>
          ))}
        </div>
      )}
      <NetCanvas
        visibleBlocks={visibleBlocks}
        visibleEdges={visibleEdges}
        unregistered={unregistered}
        hubSet={hubSet}
        colorFor={colorFor}
        onSelect={setSelected}
      />
      <span className='text-11px text-t-tertiary'>
        {t('learningGraphStructCounts', {
          blocks: visibleBlocks.length,
          spines: structure.spines.length,
        })}
        {' · '}
        {t('learningGraphStructEdgeCount', { edges: visibleEdges.length })}
        {unregistered.length > 0 ? ` · ${t('learningGraphStructUnregistered', { count: unregistered.length })}` : ''}
      </span>
      {selected && (
        <BlockDetail block={selected} structure={structure} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}

function TextLabel({ children }: { children: React.ReactNode }) {
  return <span className='text-t-secondary'>{children}</span>;
}

function FilterChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type='button'
      onClick={onClick}
      className={`cursor-pointer rounded-4px border-1 border-solid px-6px py-1px text-11px transition-colors ${
        active
          ? 'border-[var(--color-primary-6)] bg-[var(--color-primary-1)] text-[var(--color-primary-6)]'
          : 'border-[var(--color-border-2)] bg-transparent text-[var(--color-text-2)] hover:bg-[var(--color-fill-1)]'
      }`}
    >
      {children}
    </button>
  );
}

function NetCanvas({
  visibleBlocks,
  visibleEdges,
  unregistered,
  hubSet,
  colorFor,
  onSelect,
}: {
  visibleBlocks: BlockView[];
  visibleEdges: StructureEdgeView[];
  unregistered: string[];
  hubSet: Set<string>;
  colorFor: (spine: string) => string;
  onSelect: (block: BlockView) => void;
}) {
  const { pos, width, height } = useMemo(() => {
    const graph = new dagre.graphlib.Graph();
    graph.setGraph({ rankdir: 'LR', nodesep: 12, ranksep: 60, marginx: 16, marginy: 16 });
    graph.setDefaultEdgeLabel(() => ({}));
    for (const block of visibleBlocks) {
      graph.setNode(block.name, { width: W, height: H });
    }
    for (const name of unregistered) {
      graph.setNode(`◇${name}`, { width: W, height: H });
    }
    for (const edge of visibleEdges) {
      // **方向反转**：assumes 的门语义——让前置排在左（先学侧）。
      const [from, to] = edgeEndpoints(edge);
      graph.setEdge(from, to);
    }
    dagre.layout(graph);
    const points = new Map<string, { x: number; y: number }>();
    for (const block of visibleBlocks) {
      const point = graph.node(block.name) as { x: number; y: number } | undefined;
      points.set(block.name, { x: point?.x ?? 0, y: point?.y ?? 0 });
    }
    for (const name of unregistered) {
      const point = graph.node(`◇${name}`) as { x: number; y: number } | undefined;
      points.set(`◇${name}`, { x: point?.x ?? 0, y: point?.y ?? 0 });
    }
    const xs = [...points.values()].map((point) => point.x);
    const ys = [...points.values()].map((point) => point.y);
    return {
      pos: points,
      width: (xs.length ? Math.max(...xs) : 0) + W / 2 + 16,
      height: (ys.length ? Math.max(...ys) : 0) + H / 2 + 16,
    };
  }, [visibleBlocks, visibleEdges, unregistered]);

  const edgeKey = (edge: StructureEdgeView) =>
    `${edge.source}|${edge.target}|${edge.edge_type}`;

  return (
    <div className='max-h-480px overflow-auto rounded-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
      <div className='relative origin-top-left' style={{ width, height }}>
        <svg className='absolute inset-0' width={width} height={height} style={{ pointerEvents: 'none' }}>
          <defs>
            {Object.entries(EDGE_COLORS).map(([type, color]) => (
              <marker
                key={type}
                id={`net-arrow-${type}`}
                viewBox='0 0 10 10'
                refX='9'
                refY='5'
                markerWidth='6'
                markerHeight='6'
                orient='auto-start-reverse'
              >
                <path d='M 0 0 L 10 5 L 0 10 z' fill={color} />
              </marker>
            ))}
          </defs>
          {visibleEdges.map((edge) => {
            // **边方向反转：assumes = 门**（`edgeEndpoints` 单源）——箭头从
            // 「前置」指向「需求方」；composes 方向保持；contrasts 虚线无箭头。
            const [fromName, toName] = edgeEndpoints(edge);
            const from = pos.get(fromName) ?? pos.get(`◇${fromName}`);
            const to = pos.get(toName) ?? pos.get(`◇${toName}`);
            if (!from || !to) return null;
            const color = EDGE_COLORS[edge.edge_type] ?? '#94a3b8';
            return (
              <line
                key={edgeKey(edge)}
                x1={from.x + (from.x <= to.x ? W / 2 : -W / 2)}
                y1={from.y}
                x2={to.x + (from.x <= to.x ? -W / 2 : W / 2)}
                y2={to.y}
                stroke={color}
                strokeWidth={edge.edge_type === 'composes' ? 2.2 : 1.4}
                strokeDasharray={edge.edge_type === 'contrasts' ? '5 3' : undefined}
                markerEnd={
                  edge.edge_type === 'contrasts' ? undefined : `url(#net-arrow-${edge.edge_type})`
                }
              >
                <title>
                  {edge.source} → {edge.target}
                  {edge.note.trim() !== '' ? ` — ${edge.note}` : ''}
                </title>
              </line>
            );
          })}
        </svg>
        {visibleBlocks.map((block) => {
          const point = pos.get(block.name);
          if (!point) return null;
          const isHub = hubSet.has(block.name);
          return (
            <button
              key={block.name}
              type='button'
              title={block.note.trim() !== '' ? block.note : block.name}
              className='absolute flex cursor-pointer items-center justify-center overflow-hidden rounded-6px px-4px text-10px leading-tight hover:brightness-125'
              style={{
                left: point.x - W / 2,
                top: point.y - H / 2,
                width: W,
                height: H,
                border: isHub
                  ? `2.2px solid ${HUB_COLOR}`
                  : `1.5px solid ${colorFor(block.spine)}`,
                background: 'var(--color-bg-2)',
                color: 'var(--color-text-1)',
              }}
              onClick={() => onSelect(block)}
            >
              <span className='truncate'>{block.name}</span>
            </button>
          );
        })}
        {unregistered.map((name) => {
          const point = pos.get(`◇${name}`);
          if (!point) return null;
          return (
            <div
              key={`◇${name}`}
              className='absolute flex items-center justify-center overflow-hidden px-4px text-10px leading-tight'
              style={{
                left: point.x - W / 2,
                top: point.y - H / 2,
                width: W,
                height: H,
                border: '1.2px dashed #94a3b8',
                borderRadius: 2,
                transform: 'rotate(0deg)',
                background: 'var(--color-bg-2)',
                color: 'var(--color-text-3)',
              }}
            >
              <span className='truncate'>◇{name}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
