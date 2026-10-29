/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Mindmap 布局纯函数护栏（k01）：锁死「所有节点必须在画布内」与
 * 「两翼不压中心主题框」。回归背景：旧版两翼用绝对坐标（side*SPINE_X /
 * side*BLOCK_X），-1 侧整翼落负坐标被 SVG 视口裁剪不可见，且中心主题框
 * 与 +1 侧翼首列块横向重叠。
 */

import { describe, expect, test } from 'bun:test';
import type { BlockView, SpineView, StructureView } from '../../types';
import {
  BLOCK_H,
  BLOCK_W,
  CENTER_TOPIC,
  CENTER_X,
  SPINE_NODE_H,
  SPINE_NODE_W,
  computeLayout,
} from './mindmapLayout';

const makeSpine = (name: string): SpineView => ({
  name,
  definition: `${name} definition`,
  key_points: [],
  instance: '',
  thin_spine: null,
  reason: '',
});

const makeBlock = (spine: string, index: number): BlockView => ({
  name: `${spine}-b${index}`,
  slug: `${spine}-b${index}`,
  spine,
  stage: '入门',
  note: '',
  layer: 0,
  no_prereq: true,
  door_refs: [],
});

const makeStructure = (spineBlocks: [string, number][]): Pick<StructureView, 'spines' | 'blocks'> => ({
  spines: spineBlocks.map(([name]) => makeSpine(name)),
  blocks: spineBlocks.flatMap(([name, count]) =>
    Array.from({ length: count }, (_, index) => makeBlock(name, index))
  ),
});

/** 布局输出的所有矩形（主干框 / 块框 / 中心主题框），以 [minX, maxX]×[minY, maxY] 计。 */
const layoutRects = (layout: ReturnType<typeof computeLayout>) => {
  const rects: { tag: string; minX: number; maxX: number; minY: number; maxY: number }[] = [
    {
      tag: 'center',
      minX: CENTER_TOPIC.x,
      maxX: CENTER_TOPIC.x + CENTER_TOPIC.w,
      minY: CENTER_TOPIC.y,
      maxY: CENTER_TOPIC.y + CENTER_TOPIC.h,
    },
  ];
  for (const group of layout.groups) {
    rects.push({
      tag: `spine:${group.spine}`,
      minX: group.spineX - SPINE_NODE_W / 2,
      maxX: group.spineX + SPINE_NODE_W / 2,
      minY: group.spineY - SPINE_NODE_H / 2,
      maxY: group.spineY + SPINE_NODE_H / 2,
    });
    for (const placed of group.blocks) {
      rects.push({
        tag: `block:${placed.block.name}`,
        minX: placed.x - BLOCK_W / 2,
        maxX: placed.x + BLOCK_W / 2,
        minY: placed.y - BLOCK_H / 2,
        maxY: placed.y + BLOCK_H / 2,
      });
    }
  }
  return rects;
};

describe('mindmap computeLayout', () => {
  test('≥2 主干：两翼非空，全部矩形落在 [0,width]×[0,height] 画布内', () => {
    // 3/2/1 块 → 配平后 3 块主干一侧、2+1 块主干另一侧，两翼均非空。
    const structure = makeStructure([
      ['alpha', 3],
      ['beta', 2],
      ['gamma', 1],
    ]);
    const layout = computeLayout(structure);

    const sides = new Set(layout.groups.map((group) => group.side));
    expect(sides.has(1)).toBe(true);
    expect(sides.has(-1)).toBe(true);

    const { width, height } = layout;
    expect(width).toBeGreaterThan(0);
    expect(height).toBeGreaterThan(0);
    for (const rect of layoutRects(layout)) {
      expect(rect.minX).toBeGreaterThanOrEqual(0);
      expect(rect.maxX).toBeLessThanOrEqual(width);
      expect(rect.minY).toBeGreaterThanOrEqual(0);
      expect(rect.maxY).toBeLessThanOrEqual(height);
    }
  });

  test('两翼与中心主题框互不横向重叠（左翼在左、右翼在右）', () => {
    const structure = makeStructure([
      ['alpha', 3],
      ['beta', 2],
      ['gamma', 1],
    ]);
    const layout = computeLayout(structure);
    const centerMinX = CENTER_TOPIC.x;
    const centerMaxX = CENTER_TOPIC.x + CENTER_TOPIC.w;

    const wingExtent = (side: 1 | -1) => {
      const wings = layout.groups.filter((group) => group.side === side);
      expect(wings.length).toBeGreaterThan(0);
      const xs = wings.flatMap((group) => [
        group.spineX - SPINE_NODE_W / 2,
        group.spineX + SPINE_NODE_W / 2,
        ...group.blocks.flatMap((placed) => [placed.x - BLOCK_W / 2, placed.x + BLOCK_W / 2]),
      ]);
      return { minX: Math.min(...xs), maxX: Math.max(...xs) };
    };

    // side=-1 画在画布左半（CENTER_X 之左），side=+1 画在右半。
    expect(CENTER_X).toBeGreaterThan(0);
    expect(wingExtent(-1).maxX).toBeLessThan(centerMinX);
    expect(wingExtent(1).minX).toBeGreaterThan(centerMaxX);
  });

  test('1 主干：单翼也完整落画布内且不压中心框', () => {
    const structure = makeStructure([['solo', 2]]);
    const layout = computeLayout(structure);
    const { width, height } = layout;

    for (const rect of layoutRects(layout)) {
      expect(rect.minX).toBeGreaterThanOrEqual(0);
      expect(rect.maxX).toBeLessThanOrEqual(width);
      expect(rect.minY).toBeGreaterThanOrEqual(0);
      expect(rect.maxY).toBeLessThanOrEqual(height);
    }
    const centerMinX = CENTER_TOPIC.x;
    const centerMaxX = CENTER_TOPIC.x + CENTER_TOPIC.w;
    for (const group of layout.groups) {
      const xs = [
        group.spineX - SPINE_NODE_W / 2,
        group.spineX + SPINE_NODE_W / 2,
        ...group.blocks.flatMap((placed) => [placed.x - BLOCK_W / 2, placed.x + BLOCK_W / 2]),
      ];
      // 单翼整翼在中心框某一侧，不允许横跨/重叠。
      const minX = Math.min(...xs);
      const maxX = Math.max(...xs);
      const strictlyLeft = maxX < centerMinX;
      const strictlyRight = minX > centerMaxX;
      expect(strictlyLeft || strictlyRight).toBe(true);
    }
  });
});
