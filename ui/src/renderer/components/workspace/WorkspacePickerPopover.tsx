import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

const VIEWPORT_MARGIN = 8;
const POPOVER_GAP = 6;
const MIN_AVAILABLE_HEIGHT = 120;

/**
 * A body-level layer for workspace pickers.
 *
 * Workspace selection is reachable from the sidebar as well as page-level
 * controls. Rendering this layer below `document.body` keeps it out of page
 * stacking contexts (Composer, cards, and empty states), while its trigger
 * continues to own the actual workspace-selection behaviour.
 */
export const WORKSPACE_PICKER_POPOVER_Z_INDEX = 10020;

export type WorkspacePickerPopoverPlacement = 'above' | 'below';

export type WorkspacePickerPopoverProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  triggerRef: React.RefObject<HTMLElement | null>;
  preferredPlacement?: WorkspacePickerPopoverPlacement;
  minWidth?: number;
  maxWidth?: number;
  matchTriggerWidth?: boolean;
  className?: string;
  children: React.ReactNode;
  testId?: string;
};

export type WorkspacePickerPopoverPosition = {
  left: number;
  top?: number;
  bottom?: number;
  width: number;
  maxHeight: number;
};

type WorkspacePickerViewport = {
  width: number;
  height: number;
};

const currentViewport = (): WorkspacePickerViewport => ({
  width: window.innerWidth || document.documentElement.clientWidth,
  height: window.innerHeight || document.documentElement.clientHeight,
});

export const resolveWorkspacePickerPopoverPosition = (
  trigger: DOMRect,
  preferredPlacement: WorkspacePickerPopoverPlacement,
  minWidth: number,
  maxWidth: number,
  matchTriggerWidth: boolean,
  viewport: WorkspacePickerViewport = currentViewport(),
): WorkspacePickerPopoverPosition => {
  const viewportWidth = viewport.width;
  const viewportHeight = viewport.height;
  const width = Math.min(
    Math.max(viewportWidth - VIEWPORT_MARGIN * 2, 0),
    Math.max(matchTriggerWidth ? trigger.width : minWidth, minWidth),
    maxWidth,
  );
  const left = Math.min(Math.max(trigger.left, VIEWPORT_MARGIN), Math.max(VIEWPORT_MARGIN, viewportWidth - width - VIEWPORT_MARGIN));
  const belowSpace = Math.max(0, viewportHeight - trigger.bottom - VIEWPORT_MARGIN - POPOVER_GAP);
  const aboveSpace = Math.max(0, trigger.top - VIEWPORT_MARGIN - POPOVER_GAP);
  const preferAbove = preferredPlacement === 'above';
  const openAbove = preferAbove
    ? aboveSpace >= MIN_AVAILABLE_HEIGHT || aboveSpace > belowSpace
    : belowSpace < MIN_AVAILABLE_HEIGHT && aboveSpace > belowSpace;
  const availableHeight = openAbove ? aboveSpace : belowSpace;

  return {
    left,
    width,
    maxHeight: availableHeight,
    ...(openAbove
      ? { bottom: Math.max(VIEWPORT_MARGIN, viewportHeight - trigger.top + POPOVER_GAP) }
      : { top: trigger.bottom + POPOVER_GAP }),
  };
};

const WorkspacePickerPopover: React.FC<WorkspacePickerPopoverProps> = ({
  open,
  onOpenChange,
  triggerRef,
  preferredPlacement = 'below',
  minWidth = 230,
  maxWidth = 360,
  matchTriggerWidth = false,
  className,
  children,
  testId,
}) => {
  const popoverRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<WorkspacePickerPopoverPosition | null>(null);

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    setPosition(
      resolveWorkspacePickerPopoverPosition(
        trigger.getBoundingClientRect(),
        preferredPlacement,
        minWidth,
        maxWidth,
        matchTriggerWidth,
        currentViewport(),
      ),
    );
  }, [matchTriggerWidth, maxWidth, minWidth, preferredPlacement, triggerRef]);

  const closeFromDismiss = useCallback(() => {
    onOpenChange(false);
    requestAnimationFrame(() => triggerRef.current?.focus());
  }, [onOpenChange, triggerRef]);

  useLayoutEffect(() => {
    if (open) updatePosition();
  }, [open, updatePosition]);

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!triggerRef.current?.contains(target) && !popoverRef.current?.contains(target)) {
        closeFromDismiss();
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        closeFromDismiss();
      }
    };
    const resizeObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(updatePosition);
    if (triggerRef.current) resizeObserver?.observe(triggerRef.current);

    document.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown);
    window.addEventListener('resize', updatePosition);
    window.addEventListener('scroll', updatePosition, true);
    return () => {
      resizeObserver?.disconnect();
      document.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('resize', updatePosition);
      window.removeEventListener('scroll', updatePosition, true);
    };
  }, [closeFromDismiss, open, triggerRef, updatePosition]);

  if (!open || typeof document === 'undefined' || !position) return null;

  return createPortal(
    <div
      ref={popoverRef}
      className={className}
      data-testid={testId}
      data-workspace-picker-popover='true'
      style={{
        position: 'fixed',
        left: position.left,
        top: position.top,
        bottom: position.bottom,
        width: position.width,
        maxHeight: position.maxHeight,
        zIndex: WORKSPACE_PICKER_POPOVER_Z_INDEX,
        overflow: 'auto',
        background: 'var(--color-bg-1)',
        border: '1px solid var(--color-border-2)',
        borderRadius: '12px',
        boxShadow: '0 4px 12px rgba(0,0,0,0.08), 0 12px 32px rgba(0,0,0,0.06)',
        isolation: 'isolate',
      }}
    >
      {children}
    </div>,
    document.body,
  );
};

export default WorkspacePickerPopover;
