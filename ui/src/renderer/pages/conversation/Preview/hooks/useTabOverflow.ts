/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { TAB_OVERFLOW_THRESHOLD } from '../constants';

/**
 * Tab 渐变状态
 * Tab fade state for gradient indicators
 */
export interface TabFadeState {
  /**
   * 是否显示左侧渐变指示器
   * Whether to show left gradient indicator
   */
  left: boolean;

  /**
   * 是否显示右侧渐变指示器
   * Whether to show right gradient indicator
   */
  right: boolean;
}

const maxScrollLeft = (container: HTMLDivElement) =>
  Math.max(0, container.scrollWidth - container.clientWidth);

const wheelDeltaPixels = (delta: number, deltaMode: number, lineHeight: number) => {
  if (deltaMode === WheelEvent.DOM_DELTA_LINE) return delta * lineHeight;
  if (deltaMode === WheelEvent.DOM_DELTA_PAGE) return delta * lineHeight * 16;
  return delta;
};

type UseTabOverflowOptions = {
  tabCount: number;
  activeTabId: string | null;
};

/**
 * Tab 横向溢出检测 Hook
 * Hook for detecting tab horizontal overflow
 *
 * 用于显示左右渐变指示器，提示用户可以滚动查看更多 Tab
 * Used to display left/right gradient indicators to prompt users that more tabs can be scrolled
 */
export const useTabOverflow = ({ tabCount, activeTabId }: UseTabOverflowOptions) => {
  const tabsContainerRef = useRef<HTMLDivElement | null>(null);
  const prevTabCountRef = useRef(tabCount);
  const [scrollerNode, setScrollerNode] = useState<HTMLDivElement | null>(null);
  const [tabFadeState, setTabFadeState] = useState<TabFadeState>({ left: false, right: false });

  const setTabsContainerRef = useCallback((node: HTMLDivElement | null) => {
    tabsContainerRef.current = node;
    setScrollerNode(node);
  }, []);

  /**
   * 更新 Tab 溢出状态
   * Update tab overflow state
   */
  const updateTabOverflow = useCallback(() => {
    const container = tabsContainerRef.current;
    if (!container) return;

    const { scrollLeft, scrollWidth, clientWidth } = container;

    // 检查是否有横向溢出（内容宽度大于容器宽度）
    // Check if there's horizontal overflow (content width exceeds container width)
    const hasOverflow = scrollWidth > clientWidth + 1;

    const nextState: TabFadeState = {
      // 左侧渐变：有溢出且已向右滚动 / Left gradient: has overflow and scrolled right
      left: hasOverflow && scrollLeft > TAB_OVERFLOW_THRESHOLD,
      // 右侧渐变：有溢出且未滚动到最右侧 / Right gradient: has overflow and not scrolled to rightmost
      right: hasOverflow && scrollLeft + clientWidth < scrollWidth - TAB_OVERFLOW_THRESHOLD,
    };

    // 只在状态变化时更新，避免不必要的重渲染 / Only update when state changes to avoid unnecessary re-renders
    setTabFadeState((prev) => {
      if (prev.left === nextState.left && prev.right === nextState.right) return prev;
      return nextState;
    });
  }, []);

  // New tabs append on the trailing edge next to the pinned +. Prefer clipping
  // from the left so that edge (and the + outside the scroller) stay visible.
  useEffect(() => {
    const container = tabsContainerRef.current;
    if (!container) return;

    const grew = tabCount > prevTabCountRef.current;
    prevTabCountRef.current = tabCount;

    const sync = () => {
      if (grew) {
        container.scrollLeft = maxScrollLeft(container);
      } else if (activeTabId) {
        const activeChip = container.querySelector<HTMLElement>('[aria-selected="true"]');
        activeChip?.scrollIntoView({ inline: 'nearest', block: 'nearest' });
      }
      updateTabOverflow();
    };

    sync();
    const raf = requestAnimationFrame(sync);
    return () => cancelAnimationFrame(raf);
  }, [activeTabId, scrollerNode, tabCount, updateTabOverflow]);

  // Attach after the scroller mounts (PreviewTabs is often not in the tree on the
  // first PreviewPanel effect pass while the panel is closed).
  useEffect(() => {
    const container = scrollerNode;
    if (!container) return;

    const handleScroll = () => updateTabOverflow();
    const handleWheel = (event: WheelEvent) => {
      const max = maxScrollLeft(container);
      if (max <= 0) return;

      const lineHeight = container.clientHeight || 28;
      const deltaX = wheelDeltaPixels(event.deltaX, event.deltaMode, lineHeight);
      const deltaY = wheelDeltaPixels(event.deltaY, event.deltaMode, lineHeight);
      // Prefer explicit horizontal deltas; otherwise map vertical wheel to x-pan.
      const delta = Math.abs(deltaX) > Math.abs(deltaY) ? deltaX : deltaY;
      if (delta === 0) return;

      const next = Math.min(max, Math.max(0, container.scrollLeft + delta));
      if (next === container.scrollLeft) return;

      event.preventDefault();
      container.scrollLeft = next;
      updateTabOverflow();
    };

    container.addEventListener('scroll', handleScroll, { passive: true });
    container.addEventListener('wheel', handleWheel, { passive: false });
    window.addEventListener('resize', updateTabOverflow);

    let resizeObserver: ResizeObserver | null = null;
    if (typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => {
        const max = maxScrollLeft(container);
        if (container.scrollLeft > max) {
          container.scrollLeft = max;
        }
        updateTabOverflow();
      });
      resizeObserver.observe(container);
    }

    updateTabOverflow();

    return () => {
      container.removeEventListener('scroll', handleScroll);
      container.removeEventListener('wheel', handleWheel);
      window.removeEventListener('resize', updateTabOverflow);
      if (resizeObserver) {
        resizeObserver.disconnect();
      }
    };
  }, [scrollerNode, updateTabOverflow]);

  return {
    tabsContainerRef: setTabsContainerRef,
    tabFadeState,
  };
};
