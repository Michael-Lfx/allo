import { useCallback, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type RefCallback } from 'react';

const CHAT_MODEL_TRIGGER_MIN_WIDTH = 28;
const CHAT_MODEL_TRIGGER_SAFE_PADDING = 12;

export type ChatModelTriggerExpansionSide = 'start' | 'end';

export type ChatModelTriggerRect = {
  left: number;
  right: number;
};

export type ChatModelTriggerBoundary = {
  left: number;
  right: number;
};

export type ChatModelTriggerPlacement = {
  side: ChatModelTriggerExpansionSide;
  width: number;
};

type ResolveChatModelTriggerPlacementOptions = {
  slotRect: ChatModelTriggerRect;
  boundary: ChatModelTriggerBoundary;
  desiredWidth: number;
  direction?: 'ltr' | 'rtl';
};

/**
 * Choose the side with enough horizontal room for the expanded model label.
 * `start` preserves the existing LTR leftward overlay; `end` is the fallback
 * used when that side would place the leading icon outside the visible area.
 */
export const resolveChatModelTriggerPlacement = ({
  slotRect,
  boundary,
  desiredWidth,
  direction = 'ltr',
}: ResolveChatModelTriggerPlacementOptions): ChatModelTriggerPlacement => {
  const availableForStart =
    direction === 'rtl' ? boundary.right - slotRect.left : slotRect.right - boundary.left;
  const availableForEnd =
    direction === 'rtl' ? slotRect.right - boundary.left : boundary.right - slotRect.left;
  const fitsStart = availableForStart >= desiredWidth;
  const fitsEnd = availableForEnd >= desiredWidth;
  const side: ChatModelTriggerExpansionSide = fitsStart
    ? 'start'
    : fitsEnd
      ? 'end'
      : availableForStart >= availableForEnd
        ? 'start'
        : 'end';
  const available = side === 'start' ? availableForStart : availableForEnd;
  const width = Math.max(
    CHAT_MODEL_TRIGGER_MIN_WIDTH,
    Math.min(desiredWidth, Math.max(0, available)),
  );

  return { side, width: Math.round(width * 100) / 100 };
};

const isHorizontalClip = (value: string): boolean =>
  value === 'hidden' || value === 'clip' || value === 'auto' || value === 'scroll' || value === 'overlay';

const visibleHorizontalBoundary = (element: HTMLElement): ChatModelTriggerBoundary => {
  const visualViewport = window.visualViewport;
  let left = visualViewport?.offsetLeft ?? 0;
  let right = left + (visualViewport?.width ?? window.innerWidth);

  for (let ancestor = element.parentElement; ancestor && ancestor !== document.body; ancestor = ancestor.parentElement) {
    const style = getComputedStyle(ancestor);
    if (!isHorizontalClip(style.overflowX) && !isHorizontalClip(style.overflowY)) continue;
    const rect = ancestor.getBoundingClientRect();
    left = Math.max(left, rect.left);
    right = Math.min(right, rect.right);
  }

  return {
    left: left + CHAT_MODEL_TRIGGER_SAFE_PADDING,
    right: right - CHAT_MODEL_TRIGGER_SAFE_PADDING,
  };
};

type UseChatModelTriggerExpansionOptions = {
  enabled?: boolean;
  compact?: boolean;
  open?: boolean;
  expandedWidth?: number;
  slotSelector?: string;
  cssVariablePrefix?: 'model' | 'strategy';
};

type UseChatModelTriggerExpansionResult = {
  ref: RefCallback<HTMLButtonElement>;
  style: CSSProperties;
  side: ChatModelTriggerExpansionSide;
};

/** Keep a narrow model trigger inside its actual visible horizontal boundary. */
export const useChatModelTriggerExpansion = ({
  enabled = true,
  compact = false,
  open = false,
  expandedWidth,
  slotSelector = '.chat-model-picker-slot',
  cssVariablePrefix = 'model',
}: UseChatModelTriggerExpansionOptions = {}): UseChatModelTriggerExpansionResult => {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const desiredWidth = expandedWidth ?? (compact ? 136 : 176);
  const [placement, setPlacement] = useState<ChatModelTriggerPlacement>({
    side: 'start',
    width: desiredWidth,
  });

  const ref = useCallback<RefCallback<HTMLButtonElement>>((element) => {
    triggerRef.current = element;
  }, []);

  const updatePlacement = useCallback(() => {
    const trigger = triggerRef.current;
    if (!enabled || !trigger) return;
    const slot = trigger.closest<HTMLElement>(slotSelector) ?? trigger;
    const slotRect = slot.getBoundingClientRect();
    const direction = getComputedStyle(trigger).direction === 'rtl' ? 'rtl' : 'ltr';
    const nextPlacement = resolveChatModelTriggerPlacement({
      slotRect,
      boundary: visibleHorizontalBoundary(trigger),
      desiredWidth,
      direction,
    });
    setPlacement((current) =>
      current.side === nextPlacement.side && current.width === nextPlacement.width ? current : nextPlacement,
    );
  }, [desiredWidth, enabled, slotSelector]);

  useLayoutEffect(() => {
    if (!enabled) return;
    updatePlacement();
    const trigger = triggerRef.current;
    if (!trigger) return;
    const slot = trigger.closest<HTMLElement>(slotSelector);
    const resizeObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(updatePlacement);
    resizeObserver?.observe(trigger);
    if (slot) resizeObserver?.observe(slot);

    window.addEventListener('resize', updatePlacement);
    window.addEventListener('scroll', updatePlacement, true);
    window.visualViewport?.addEventListener('resize', updatePlacement);
    window.visualViewport?.addEventListener('scroll', updatePlacement);
    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener('resize', updatePlacement);
      window.removeEventListener('scroll', updatePlacement, true);
      window.visualViewport?.removeEventListener('resize', updatePlacement);
      window.visualViewport?.removeEventListener('scroll', updatePlacement);
    };
  }, [enabled, open, slotSelector, updatePlacement]);

  const style = useMemo(
    () => {
      const variablePrefix = cssVariablePrefix === 'strategy' ? 'chat-strategy' : 'chat-model-picker';
      return {
        [`--${variablePrefix}-expanded-width`]: `${placement.width}px`,
        [`--${variablePrefix}-expanded-inline-start`]: placement.side === 'end' ? '0px' : 'auto',
        [`--${variablePrefix}-expanded-inline-end`]: placement.side === 'start' ? '0px' : 'auto',
      } as CSSProperties;
    },
    [cssVariablePrefix, placement],
  );

  return { ref, style, side: placement.side };
};
