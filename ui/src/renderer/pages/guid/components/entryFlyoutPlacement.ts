import { useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from 'react';
import { WORKSPACE_PICKER_POPOVER_Z_INDEX } from '@/renderer/components/workspace/WorkspacePickerPopover';

export const ENTRY_FLYOUT_WIDTH = 320;
export const ENTRY_FLYOUT_GAP = 0;
export const ENTRY_FLYOUT_MARGIN = 8;
export const ENTRY_FLYOUT_MAX_HEIGHT = 640;
const ENTRY_FLYOUT_MIN_HEIGHT = 48;

export type FlyoutBox = {
  left: number;
  width: number;
  maxHeight: number;
  top?: number;
  bottom?: number;
};

export type FlyoutViewport = {
  width: number;
  height: number;
};

const readViewport = (): FlyoutViewport => ({
  width: window.innerWidth || document.documentElement.clientWidth,
  height: window.innerHeight || document.documentElement.clientHeight,
});

const round = (value: number) => Math.round(value);

/** Bottom edge of the row that opened the submenu. */
export const entryFlyoutAlignBottom = (
  root: ParentNode | null,
  fallback: number,
  rowSelector = '[role="option"][aria-selected="true"]',
): number => {
  const row = root?.querySelector<HTMLElement>(rowSelector);
  return row?.getBoundingClientRect().bottom ?? fallback;
};

/**
 * Pin the submenu's bottom edge to the open primary row.
 * The panel grows upward and scrolls once it would pass the viewport margin,
 * so a short list stays beside its row and a tall list does not hang below it.
 */
export const measureEntryFlyout = (
  anchor: DOMRect,
  alignBottom = anchor.bottom,
  viewport: FlyoutViewport = readViewport(),
): FlyoutBox => {
  const { width: viewportWidth, height: viewportHeight } = viewport;
  const width = Math.min(ENTRY_FLYOUT_WIDTH, Math.max(0, viewportWidth - ENTRY_FLYOUT_MARGIN * 2));
  const spaceRight = viewportWidth - anchor.right - ENTRY_FLYOUT_GAP - ENTRY_FLYOUT_MARGIN;
  const spaceLeft = anchor.left - ENTRY_FLYOUT_GAP - ENTRY_FLYOUT_MARGIN;
  const placeRight = spaceRight >= width || spaceRight >= spaceLeft;
  const left = placeRight
    ? Math.min(
        Math.max(ENTRY_FLYOUT_MARGIN, anchor.right + ENTRY_FLYOUT_GAP),
        Math.max(ENTRY_FLYOUT_MARGIN, viewportWidth - width - ENTRY_FLYOUT_MARGIN),
      )
    : Math.max(ENTRY_FLYOUT_MARGIN, anchor.left - ENTRY_FLYOUT_GAP - width);

  const maxPanel = Math.min(
    ENTRY_FLYOUT_MAX_HEIGHT,
    Math.max(ENTRY_FLYOUT_MIN_HEIGHT, viewportHeight - ENTRY_FLYOUT_MARGIN * 2),
  );
  const roomAbove = alignBottom - ENTRY_FLYOUT_MARGIN;
  if (roomAbove >= ENTRY_FLYOUT_MIN_HEIGHT) {
    return {
      left: round(left),
      width: round(width),
      bottom: round(Math.max(0, viewportHeight - alignBottom)),
      maxHeight: round(Math.min(maxPanel, roomAbove)),
    };
  }

  return {
    left: round(left),
    width: round(width),
    top: ENTRY_FLYOUT_MARGIN,
    maxHeight: round(maxPanel),
  };
};

const flyoutBoxesMatch = (current: FlyoutBox | null, next: FlyoutBox) =>
  current !== null &&
  current.left === next.left &&
  current.width === next.width &&
  current.maxHeight === next.maxHeight &&
  current.top === next.top &&
  current.bottom === next.bottom;

export const entryFlyoutPositionStyle = (box: FlyoutBox | null): CSSProperties => ({
  position: 'fixed',
  left: box?.left ?? -9999,
  top: box?.bottom == null ? (box?.top ?? -9999) : undefined,
  bottom: box?.bottom,
  width: box?.width ?? ENTRY_FLYOUT_WIDTH,
  maxHeight: box?.maxHeight ?? ENTRY_FLYOUT_MAX_HEIGHT,
  visibility: box ? 'visible' : 'hidden',
  zIndex: WORKSPACE_PICKER_POPOVER_Z_INDEX,
});

export const useEntryFlyoutBox = <T extends HTMLElement>(
  anchorRef: RefObject<T | null> | undefined,
  active: boolean,
  rowSelector = '[role="option"][aria-selected="true"]',
): FlyoutBox | null => {
  const [box, setBox] = useState<FlyoutBox | null>(null);
  const updateRef = useRef<() => void>(() => undefined);

  useLayoutEffect(() => {
    updateRef.current = () => {
      if (!active) return;
      const anchor = anchorRef?.current;
      if (!anchor) return;
      const rect = anchor.getBoundingClientRect();
      const alignBottom = entryFlyoutAlignBottom(anchor, rect.bottom, rowSelector);
      const next = measureEntryFlyout(rect, alignBottom);
      setBox((current) => (flyoutBoxesMatch(current, next) ? current : next));
    };
    updateRef.current();
  });

  useLayoutEffect(() => {
    if (!active) {
      setBox(null);
      return undefined;
    }
    const onUpdate = () => updateRef.current();
    onUpdate();
    const anchor = anchorRef?.current;
    const resize = anchor && typeof ResizeObserver !== 'undefined' ? new ResizeObserver(onUpdate) : null;
    resize?.observe(anchor);
    const mutation = anchor && typeof MutationObserver !== 'undefined' ? new MutationObserver(onUpdate) : null;
    mutation?.observe(anchor, {
      attributes: true,
      subtree: true,
      attributeFilter: ['aria-selected'],
    });
    window.addEventListener('resize', onUpdate);
    window.addEventListener('scroll', onUpdate, true);
    return () => {
      resize?.disconnect();
      mutation?.disconnect();
      window.removeEventListener('resize', onUpdate);
      window.removeEventListener('scroll', onUpdate, true);
    };
  }, [active, anchorRef, rowSelector]);

  return active ? box : null;
};
