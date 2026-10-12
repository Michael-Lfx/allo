import dagre from '@dagrejs/dagre';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BlockView, StructureView } from '../../types';
import { BlockDetail } from './BlockDetail';
import { EDGE_COLORS, buildGroupPalette } from './palette';

const W = 150;
const H = 30;

/**
 * 学习路线（dagre LR，列 = assumes 前置链深度；层号不是排期）：全景 + 按主干
 * 取子图，**传递闭包自动补前导防断裂**——补上的前导以虚线框 +「前导」角标
 * 显示；防环：补前导时按已访问集截断。
 */
export function RouteMap({ structure }: { structure: StructureView }) {
  const { t } = useTranslation();
  const [spine, setSpine] = useState<string>('__all__');
  const [selected, setSelected] = useState<BlockView | null>(null);
  const colorFor = useMemo(
    () => buildGroupPalette(structure.spines.map((item) => item.name)),
    [structure.spines]
  );

  // assumes 邻接（source → 其前置集）；源 = 需求方，目标 = 前置。
  const prereqOf = useMemo(() => {
    const map = new Map<string, Set<string>>();
    for (const edge of structure.edges) {
      if (edge.edge_type !== 'assumes') continue;
      if (!map.has(edge.source)) map.set(edge.source, new Set());
      map.get(edge.source)!.add(edge.target);
    }
    return map;
  }, [structure.edges]);

  const { nodes, extraPrereqs } = useMemo(() => {
    if (spine === '__all__') {
      return { nodes: structure.blocks, extraPrereqs: new Set<string>() };
    }
    const picked = structure.blocks.filter((block) => block.spine === spine);
    const pickedNames = new Set(picked.map((block) => block.name));
    const completed = new Set<string>();
    // 自动补前导：沿途 assumes 前置（传递闭包），已访问集防环。
    const walk = (name: string, visiting: Set<string>) => {
      if (completed.has(name) || visiting.has(name)) return;
      visiting.add(name);
      for (const prereq of prereqOf.get(name) ?? []) walk(prereq, visiting);
      visiting.delete(name);
      if (!pickedNames.has(name)) completed.add(name);
    };
    for (const block of picked) walk(block.name, new Set());
    const extra = structure.blocks.filter(
      (block) => completed.has(block.name) && !pickedNames.has(block.name)
    );
    return { nodes: [...picked, ...extra], extraPrereqs: new Set(extra.map((block) => block.name)) };
  }, [spine, structure.blocks, prereqOf]);

  const nodeNames = useMemo(() => new Set(nodes.map((node) => node.name)), [nodes]);
  const edges = useMemo(
    () =>
      structure.edges.filter(
        (edge) =>
          edge.edge_type === 'assumes' &&
          nodeNames.has(edge.source) &&
          nodeNames.has(edge.target)
      ),
    [structure.edges, nodeNames]
  );

  const { points, width, height } = useMemo(() => {
    const graph = new dagre.graphlib.Graph();
    graph.setGraph({ rankdir: 'LR', nodesep: 12, ranksep: 60, marginx: 16, marginy: 16 });
    graph.setDefaultEdgeLabel(() => ({}));
    for (const node of nodes) graph.setNode(node.name, { width: W, height: H });
    // 门方向反转布局：前置排左（先学侧），需求方排右。
    for (const edge of edges) graph.setEdge(edge.target, edge.source);
    dagre.layout(graph);
    const placed = new Map<string, { x: number; y: number }>();
    for (const node of nodes) {
      const point = graph.node(node.name) as { x: number; y: number } | undefined;
      placed.set(node.name, { x: point?.x ?? 0, y: point?.y ?? 0 });
    }
    const xs = [...placed.values()].map((point) => point.x);
    const ys = [...placed.values()].map((point) => point.y);
    return {
      points: placed,
      width: (xs.length ? Math.max(...xs) : 0) + W / 2 + 16,
      height: (ys.length ? Math.max(...ys) : 0) + H / 2 + 16,
    };
  }, [nodes, edges]);

  const spineBlockCount = (name: string) =>
    structure.blocks.filter((block) => block.spine === name).length;

  return (
    <div className='flex flex-col gap-8px'>
      <div className='flex flex-wrap items-center gap-8px text-12px'>
        <span className='text-t-secondary'>{t('learningGraphStructRoute')}:</span>
        <select
          value={spine}
          onChange={(event) => setSpine(event.target.value)}
          className='rounded-4px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-8px py-2px text-12px text-[var(--color-text-1)]'
        >
          <option value='__all__'>{t('learningGraphStructRouteAll')}</option>
          {structure.spines.map((item) => (
            <option key={item.name} value={item.name}>
              {t('learningGraphStructRouteSpine', {
                name: item.name,
                count: spineBlockCount(item.name),
              })}
            </option>
          ))}
        </select>
        <span className='text-11px text-t-tertiary'>
          {t('learningGraphStructRouteCounts', {
            blocks: nodes.length,
            edges: edges.length,
          })}
          {extraPrereqs.size > 0 && (
            <> · {t('learningGraphStructRouteExtra', { extra: extraPrereqs.size })}</>
          )}
        </span>
      </div>

      <div className='max-h-480px overflow-auto rounded-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
        <div className='relative origin-top-left' style={{ width, height }}>
          <svg className='absolute inset-0' width={width} height={height} style={{ pointerEvents: 'none' }}>
            <defs>
              <marker
                id='route-arrow'
                viewBox='0 0 10 10'
                refX='9'
                refY='5'
                markerWidth='6'
                markerHeight='6'
                orient='auto-start-reverse'
              >
                <path d='M 0 0 L 10 5 L 0 10 z' fill={EDGE_COLORS.assumes} />
              </marker>
            </defs>
            {edges.map((edge) => {
              const from = points.get(edge.target);
              const to = points.get(edge.source);
              if (!from || !to) return null;
              return (
                <line
                  key={`${edge.source}->${edge.target}`}
                  x1={from.x + (from.x <= to.x ? W / 2 : -W / 2)}
                  y1={from.y}
                  x2={to.x + (from.x <= to.x ? -W / 2 : W / 2)}
                  y2={to.y}
                  stroke={EDGE_COLORS.assumes}
                  strokeWidth={1.3}
                  markerEnd='url(#route-arrow)'
                >
                  {edge.note.trim() !== '' ? <title>{edge.note}</title> : null}
                </line>
              );
            })}
          </svg>
          {nodes.map((node) => {
            const point = points.get(node.name);
            if (!point) return null;
            const isExtra = extraPrereqs.has(node.name);
            return (
              <button
                key={node.name}
                type='button'
                title={node.note.trim() !== '' ? node.note : node.name}
                className='absolute flex cursor-pointer items-center justify-center overflow-hidden rounded-6px px-4px text-10px leading-tight hover:brightness-125'
                style={{
                  left: point.x - W / 2,
                  top: point.y - H / 2,
                  width: W,
                  height: H,
                  border: isExtra
                    ? `1.2px dashed ${EDGE_COLORS.assumes}`
                    : `1.4px solid ${colorFor(node.spine)}`,
                  borderRadius: 6,
                  background: 'var(--color-bg-2)',
                  color: 'var(--color-text-1)',
                }}
                onClick={() => setSelected(node)}
              >
                <span className='truncate'>
                  {isExtra && (
                    <span
                      className='mr-4px rounded-3px px-3px text-8px'
                      style={{ background: 'var(--color-primary-1)', color: EDGE_COLORS.assumes }}
                    >
                      {t('learningGraphStructPrereqTag')}
                    </span>
                  )}
                  {node.name}
                </span>
              </button>
            );
          })}
        </div>
      </div>
      {selected && (
        <BlockDetail block={selected} structure={structure} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}
