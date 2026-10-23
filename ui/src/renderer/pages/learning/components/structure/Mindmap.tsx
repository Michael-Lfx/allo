import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BlockView, StructureView } from '../../types';
import { BlockDetail } from './BlockDetail';
import {
  BLOCK_H,
  BLOCK_W,
  CENTER_TOPIC,
  CENTER_X,
  SPINE_NODE_H,
  SPINE_NODE_W,
  computeLayout,
} from './mindmapLayout';
import { buildGroupPalette } from './palette';

/**
 * 思维导图（两翼配平总览）：主题居中，主干按块数降序分左右两翼，块向两侧
 * 展开。纯自绘 SVG（无图形库依赖）；主干分组配色与图谱/路线共用一源。
 * 布局几何由 mindmapLayout.ts 纯函数产出（两翼以 CENTER_X 中心对称，
 * 全部节点落画布内——单测锁死）。
 */
export function Mindmap({ structure }: { structure: StructureView }) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<BlockView | null>(null);
  const colorFor = useMemo(
    () => buildGroupPalette(structure.spines.map((spine) => spine.name)),
    [structure.spines]
  );
  const layout = useMemo(() => computeLayout(structure), [structure]);

  const { width, height } = layout;
  /** 中心主题框纵向中线（主题文本与主干→主题边的锚点）。 */
  const centerMidY = CENTER_TOPIC.y + CENTER_TOPIC.h / 2;

  return (
    <div className='flex flex-col gap-8px'>
      <div className='max-h-560px overflow-auto rounded-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
        <svg width={width} height={height} style={{ display: 'block' }}>
          {/* 中心主题 */}
          <g>
            <rect
              x={CENTER_TOPIC.x}
              y={CENTER_TOPIC.y}
              width={CENTER_TOPIC.w}
              height={CENTER_TOPIC.h}
              rx={10}
              fill='var(--color-primary-6)'
              opacity={0.92}
            />
            <text
              x={CENTER_X}
              y={centerMidY}
              textAnchor='middle'
              dy='0.35em'
              fill='#fff'
              fontSize={12}
              fontWeight={700}
            >
              {structure.scalars.goal.trim().slice(0, 14) || t('learningGraphStructTheme')}
            </text>
          </g>
          {layout.groups.map((group) => {
            const color = colorFor(group.spine);
            return (
              <g key={`${group.side}:${group.spine}`}>
                {/* 主干 → 主题 */}
                <path
                  d={`M ${CENTER_X} ${centerMidY} C ${CENTER_X + (group.spineX - CENTER_X) * 0.4} ${centerMidY}, ${
                    CENTER_X + (group.spineX - CENTER_X) * 0.6
                  } ${group.spineY}, ${
                    group.side === 1 ? group.spineX - SPINE_NODE_W / 2 : group.spineX + SPINE_NODE_W / 2
                  } ${group.spineY}`}
                  stroke={color}
                  strokeWidth={2.2}
                  fill='none'
                  opacity={0.85}
                />
                {/* 块 → 主干 */}
                {group.blocks.map((placed) => (
                  <path
                    key={placed.block.name}
                    d={`M ${
                      group.side === 1 ? group.spineX + SPINE_NODE_W / 2 : group.spineX - SPINE_NODE_W / 2
                    } ${group.spineY} C ${
                      group.side === 1 ? placed.x - BLOCK_W / 2 - 40 : placed.x + BLOCK_W / 2 + 40
                    } ${group.spineY}, ${
                      group.side === 1 ? placed.x - BLOCK_W / 2 - 40 : placed.x + BLOCK_W / 2 + 40
                    } ${placed.y}, ${
                      group.side === 1 ? placed.x - BLOCK_W / 2 : placed.x + BLOCK_W / 2
                    } ${placed.y}`}
                    stroke={color}
                    strokeWidth={1.1}
                    fill='none'
                    opacity={0.55}
                  />
                ))}
                {/* 主干节点 */}
                <g>
                  <rect
                    x={group.spineX - SPINE_NODE_W / 2}
                    y={group.spineY - SPINE_NODE_H / 2}
                    width={SPINE_NODE_W}
                    height={SPINE_NODE_H}
                    rx={8}
                    fill={color}
                    opacity={0.92}
                  />
                  <text
                    x={group.spineX}
                    y={group.spineY}
                    textAnchor='middle'
                    dy='0.35em'
                    fill='#fff'
                    fontSize={11}
                    fontWeight={600}
                  >
                    {group.spine.slice(0, 10)}
                  </text>
                </g>
                {/* 块节点 */}
                {group.blocks.map((placed) => (
                  <g key={placed.block.name}>
                    <rect
                      x={placed.x - BLOCK_W / 2}
                      y={placed.y - BLOCK_H / 2}
                      width={BLOCK_W}
                      height={BLOCK_H}
                      rx={6}
                      fill='var(--color-bg-2)'
                      stroke={color}
                      strokeWidth={1}
                      style={{ cursor: 'pointer' }}
                      onClick={() => setSelected(placed.block)}
                    />
                    <text
                      x={placed.x}
                      y={placed.y}
                      textAnchor='middle'
                      dy='0.35em'
                      fill='var(--color-text-1)'
                      fontSize={10}
                      style={{ cursor: 'pointer' }}
                      onClick={() => setSelected(placed.block)}
                    >
                      {placed.block.name.slice(0, 12)}
                    </text>
                  </g>
                ))}
              </g>
            );
          })}
        </svg>
      </div>
      <span className='text-11px text-t-tertiary'>
        {t('learningGraphStructCounts', {
          blocks: structure.blocks.length,
          spines: structure.spines.length,
        })}
      </span>
      {selected && (
        <BlockDetail block={selected} structure={structure} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}
