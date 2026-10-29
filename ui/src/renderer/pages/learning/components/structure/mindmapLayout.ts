/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { BlockView, StructureView } from '../../types';

export const SPINE_NODE_W = 128;
export const SPINE_NODE_H = 34;
export const BLOCK_W = 150;
export const BLOCK_H = 26;
const BLOCK_GAP = 6;
/** 中心主题到主干列 / 主干列到块列的水平距离。 */
const SPINE_X = 250;
const BLOCK_X = 520;
/**
 * 中心主题 x：两翼坐标以它为原点镜像。
 * 曾经两翼用绝对坐标（side*SPINE_X / side*BLOCK_X），-1 侧整翼落负坐标被
 * SVG 视口裁剪——节点不可见、只剩从画布外扫进来的边；且中心主题框（±90）
 * 与 +1 侧翼块列横向重叠。改中心对称后两翼都在画布内，主题框与两翼块列
 * 各留 SPINE_X+SPINE_NODE_W/2 以上的间隙（learnhub a928e46 同款修法）。
 */
export const CENTER_X = BLOCK_X + BLOCK_W / 2 + 40;

/** 中心主题框几何（渲染与布局单测共用一源）。 */
export const CENTER_TOPIC = { x: CENTER_X - 90, y: 18, w: 180, h: 44 } as const;

export interface MindmapLayoutGroup {
  spineX: number;
  spineY: number;
  side: 1 | -1;
  spine: string;
  blocks: { x: number; y: number; block: BlockView }[];
}

/**
 * 思维导图布局纯函数（Mindmap.tsx 只消费不算）：主干按块数降序分左右两翼
 * （重的一侧先放），翼内保持结构源顺序；两翼以 CENTER_X 中心对称展开。
 * 颜色不在布局内（由渲染侧 palette 按 spine 名取色）。
 */
export function computeLayout(
  structure: Pick<StructureView, 'spines' | 'blocks'>
): { groups: MindmapLayoutGroup[]; width: number; height: number } {
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
  // 翼内按结构源顺序重排
  const byOrder = (a: { order: number }, b: { order: number }) => a.order - b.order;
  left.sort(byOrder);
  right.sort(byOrder);

  const groups: MindmapLayoutGroup[] = [];
  const placeSide = (side: 1 | -1, spines: typeof left) => {
    let y = 52;
    for (const spine of spines) {
      const spineY = y + (spine.blocks.length * (BLOCK_H + BLOCK_GAP) - BLOCK_GAP) / 2;
      const placedBlocks = spine.blocks.map((block, index) => ({
        x: CENTER_X + side * BLOCK_X,
        y: y + index * (BLOCK_H + BLOCK_GAP) + BLOCK_H / 2,
        block,
      }));
      groups.push({
        spineX: CENTER_X + side * SPINE_X,
        spineY,
        side,
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
    ...groups.map((group) => {
      const lastBlockBottom =
        group.blocks.length > 0
          ? group.blocks[group.blocks.length - 1].y + BLOCK_H / 2
          : group.spineY - SPINE_NODE_H / 2;
      // 无块主干也要盖住主干框下沿，否则末组无块时框底被裁。
      return Math.max(lastBlockBottom, group.spineY + SPINE_NODE_H / 2);
    })
  );
  const width = CENTER_X * 2;
  return { groups, width, height };
}
