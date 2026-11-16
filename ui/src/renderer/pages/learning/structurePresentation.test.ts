/** structurePresentation.ts 纯决策单测（c5）：水位判定 + 存取往返 + 损坏容
 * 错。存储注入内存 store——不依赖 localStorage 全局（bun test 无 DOM）。 */
import { describe, expect, test } from 'bun:test';
import {
  loadStructurePresentedWatermark,
  saveStructurePresentedWatermark,
  shouldPresentStructure,
  type WatermarkStore,
} from './structurePresentation';

const memoryStore = (): WatermarkStore => {
  const map = new Map<string, string>();
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
};

describe('shouldPresentStructure', () => {
  test('无水位 + 有结构 = 呈现（首次）', () => {
    expect(shouldPresentStructure(null, 1000)).toBe(true);
  });

  test('水位落后于结构 = 呈现（重生成后值得再看一次）', () => {
    expect(shouldPresentStructure(1000, 2000)).toBe(true);
  });

  test('水位追平结构 = 不呈现（同一次生成不重复抢焦点）', () => {
    expect(shouldPresentStructure(2000, 2000)).toBe(false);
  });

  test('结构时间戳缺失/非法 = 不呈现', () => {
    expect(shouldPresentStructure(null, null)).toBe(false);
    expect(shouldPresentStructure(null, 0)).toBe(false);
    expect(shouldPresentStructure(null, -5)).toBe(false);
  });
});

describe('watermark storage', () => {
  test('读未写过 = null', () => {
    const store = memoryStore();
    expect(loadStructurePresentedWatermark('course-a', store)).toBeNull();
  });

  test('写后读回同一课；不串其它课', () => {
    const store = memoryStore();
    saveStructurePresentedWatermark('course-a', 1234, store);
    expect(loadStructurePresentedWatermark('course-a', store)).toBe(1234);
    expect(loadStructurePresentedWatermark('course-b', store)).toBeNull();
  });

  test('同课覆写取新值', () => {
    const store = memoryStore();
    saveStructurePresentedWatermark('course-a', 1, store);
    saveStructurePresentedWatermark('course-a', 2, store);
    expect(loadStructurePresentedWatermark('course-a', store)).toBe(2);
  });

  test('损坏 JSON = 无水位不抛错', () => {
    const store = memoryStore();
    store.setItem('learning.structurePresented', '{not json');
    expect(loadStructurePresentedWatermark('course-a', store)).toBeNull();
  });

  test('形状不对（数组/非数值） = 无水位不抛错', () => {
    const store = memoryStore();
    store.setItem('learning.structurePresented', '[1,2]');
    expect(loadStructurePresentedWatermark('course-a', store)).toBeNull();
    store.setItem('learning.structurePresented', '{"course-a":"NaN"}');
    expect(loadStructurePresentedWatermark('course-a', store)).toBeNull();
  });
});
