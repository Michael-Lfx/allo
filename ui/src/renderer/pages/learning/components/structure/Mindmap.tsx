import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BlockView, StructureView } from '../../types';
import { BlockDetail } from './BlockDetail';
import { buildGroupPalette } from './palette';

const SPINE_NODE_W = 128;
const SPINE_NODE_H = 34;
const BLOCK_W = 150;
const BLOCK_H = 26;
const BLOCK_GAP = 6;
/** 中心主题到主干列 / 主干列到块列的水平距离。 */
const SPINE_X = 250;
const BLOCK_X = 520;

/**
 * 思维导图（两翼配平总览）：主题居中，主干按块数降序分左右两翼，块向两侧
 * 展开。纯自绘 SVG（无图形库依赖）；主干分组配色与图谱/路线共用一源。
 */
export function Mindmap({ structure }: { structure: StructureView }) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<BlockView | null>(null);
  const colorFor = useMemo(
    () => buildGroupPalette(structure.spines.map((spine) => spine.name)),
    [structure.spines]
  );

  const layout = useMemo(() => {
    // 主干按块数降序分左右两翼（重的一侧先放），翼内保持结构源顺序。
    const withCount = structure.spines.map((spine, order) => ({
      name: spine.name,
      order,
      blocks: structure.blocks.filter((block) => block.spine === spine.name),
    }));
    const sorted = [...withCount].sort((a, b) => b.blocks.length - a.blocks.length);
    const left: typeof withCount = [];
    const right: typeof withCount = [];
    let leftWeight = 0;
    let rightWeight = 0;
    for (const spine of sorted) {
      if (leftWeight <= rightWeight) {
        left.push(spine);
        leftWeight += spine.blocks.length + 1;
      } else {
        right.push(spine);
        rightWeight += spine.blocks.length + 1;
      }
    }
    const byOrder = (a: { order: number }, b: { order: number }) => a.order - b.order;
    left.sort(byOrder);
    right.sort(byOrder);

    type Placed = {
      spineX: number;
      spineY: number;
      side: 1 | -1;
      color: string;
      spine: string;
      blocks: { x: number; y: number; block: BlockView }[];
    };
    const groups: Placed[] = [];
    const placeSide = (side: 1 | -1, spines: typeof left) => {
      let y = 52;
      for (const spine of spines) {
        const spineY = y + (spine.blocks.length * (BLOCK_H + BLOCK_GAP) - BLOCK_GAP) / 2;
        const placedBlocks = spine.blocks.map((block, index) => ({
          x: side * BLOCK_X,
          y: y + index * (BLOCK_H + BLOCK_GAP) + BLOCK_H / 2,
          block,
        }));
        groups.push({
          spineX: side * SPINE_X,
          spineY,
          side,
          color: colorFor(spine.name),
          spine: spine.name,
          blocks: placedBlocks,
        });
        y += spine.blocks.length * (BLOCK_H + BLOCK_GAP) + 36;
      }
    };
    placeSide(1, left);
    placeSide(-1, right);
    const height = Math.max(
      240,
      ...groups.map((group) =>
        group.blocks.length > 0
          ? group.blocks[group.blocks.length - 1].y + BLOCK_H
          : group.spineY
      )
    );
    return { groups, height };
  }, [structure, colorFor]);

  const width = BLOCK_X * 2 + BLOCK_W + 40;
  const centerX = width / 2;

  return (
    <div className='flex flex-col gap-8px'>
      <div className='max-h-560px overflow-auto rounded-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
        <svg width={width} height={layout.height} style={{ display: 'block' }}>
          {/* 中心主题 */}
          <g>
            <rect
              x={centerX - 90}
              y={18}
              width={180}
              height={44}
              rx={10}
              fill='var(--color-primary-6)'
              opacity={0.92}
            />
            <text
              x={centerX}
              y={40}
              textAnchor='middle'
              dominantBaseline='central'
              fill='#fff'
              fontSize={12}
              fontWeight={700}
            >
              {structure.scalars.goal.trim().slice(0, 14) || t('learningGraphStructTheme')}
            </text>
          </g>
          {layout.groups.map((group) => (
            <g key={`${group.side}:${group.spine}`}>
              {/* 主干 → 主题 */}
              <path
                d={`M ${centerX} 40 C ${centerX + (group.spineX - centerX) * 0.4} 40, ${
                  centerX + (group.spineX - centerX) * 0.6
                } ${group.spineY}, ${
                  group.side === 1 ? group.spineX - SPINE_NODE_W / 2 : group.spineX + SPINE_NODE_W / 2
                } ${group.spineY}`}
                stroke={group.color}
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
                  stroke={group.color}
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
                  fill={group.color}
                  opacity={0.92}
                />
                <text
                  x={group.spineX}
                  y={group.spineY}
                  textAnchor='middle'
                  dominantBaseline='central'
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
                    stroke={group.color}
                    strokeWidth={1}
                    style={{ cursor: 'pointer' }}
                    onClick={() => setSelected(placed.block)}
                  />
                  <text
                    x={placed.x}
                    y={placed.y}
                    textAnchor='middle'
                    dominantBaseline='central'
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
          ))}
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
