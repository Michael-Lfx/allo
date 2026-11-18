/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 头部折叠持久层存取测试（a4：存储键升版 v2 一次性清 v1 旧偏好）。
 * 存储注入内存 store（bun test 宿主无可靠 localStorage——历史波实证）；
 * 另覆盖 removeItem 缺省的旧注入方兼容臂。 */
import { describe, expect, test } from 'bun:test';
import {
  loadGraphHeaderCollapsed,
  saveGraphHeaderCollapsed,
  type HeaderCollapseStore,
} from './headerCollapse';
import {
  GRAPH_HEADER_COLLAPSED_KEY,
  LEGACY_GRAPH_HEADER_COLLAPSED_KEY,
} from './constants';

function memoryStore(): HeaderCollapseStore & { map: Map<string, string> } {
  const map = new Map<string, string>();
  return {
    map,
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
    removeItem: (key) => {
      map.delete(key);
    },
  };
}

describe('headerCollapse 存取层（a4 键升版 v2）', () => {
  test('v2 键缺席 + v1 旧偏好在场：读侧作废旧偏好（返回展开）并清理 v1 键', () => {
    const store = memoryStore();
    store.map.set(LEGACY_GRAPH_HEADER_COLLAPSED_KEY, '{"course-a":true}');
    expect(loadGraphHeaderCollapsed('course-a', store)).toBe(false);
    expect(store.map.has(LEGACY_GRAPH_HEADER_COLLAPSED_KEY)).toBe(false);
  });

  test('v2 键在场：旧键不参与判读（v2 为唯一事实源）', () => {
    const store = memoryStore();
    store.map.set(LEGACY_GRAPH_HEADER_COLLAPSED_KEY, '{"course-a":true}');
    store.map.set(GRAPH_HEADER_COLLAPSED_KEY, '{"course-b":true}');
    expect(loadGraphHeaderCollapsed('course-a', store)).toBe(false);
    expect(loadGraphHeaderCollapsed('course-b', store)).toBe(true);
    expect(store.map.has(LEGACY_GRAPH_HEADER_COLLAPSED_KEY)).toBe(true);
  });

  test('读写往返：save → load 同课折叠/异课展开（v2 语义不回归）', () => {
    const store = memoryStore();
    saveGraphHeaderCollapsed('course-a', true, store);
    expect(loadGraphHeaderCollapsed('course-a', store)).toBe(true);
    expect(loadGraphHeaderCollapsed('course-b', store)).toBe(false);
    saveGraphHeaderCollapsed('course-a', false, store);
    expect(loadGraphHeaderCollapsed('course-a', store)).toBe(false);
  });

  test('注入方缺 removeItem：读侧不抛错、作废清理跳过（旧注入方兼容臂）', () => {
    const map = new Map<string, string>();
    map.set(LEGACY_GRAPH_HEADER_COLLAPSED_KEY, '{"course-a":true}');
    const legacyStore: HeaderCollapseStore = {
      getItem: (key) => map.get(key) ?? null,
      setItem: (key, value) => {
        map.set(key, value);
      },
    };
    expect(loadGraphHeaderCollapsed('course-a', legacyStore)).toBe(false);
  });
});
