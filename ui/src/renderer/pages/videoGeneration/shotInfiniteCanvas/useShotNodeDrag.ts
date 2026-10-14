import { useEffect, useRef, type RefObject } from 'react';

import { applyCanvasNodeDragPreview } from '@oc/lib/canvas/canvas-live-viewport';
import type { CanvasNodeData, Position, ViewportTransform } from '@oc/types/canvas';

const DRAG_THRESHOLD_PX = 3;

const IGNORE_SELECTOR = [
  '[data-canvas-no-zoom]',
  '[data-canvas-context-menu]',
  '[data-connection-create-menu]',
  '[data-canvas-connection-rail]',
  '.canvas-node-resize',
  'button',
  'input',
  'textarea',
  'select',
  'label',
  'a',
].join(',');

type DragSession = {
  id: string;
  pointerId: number;
  startX: number;
  startY: number;
  origin: Position;
  moved: boolean;
  canMove: boolean;
};

type UseShotNodeDragOptions = {
  containerRef: RefObject<HTMLDivElement | null>;
  nodesRef: RefObject<CanvasNodeData[]>;
  viewportRef: RefObject<ViewportTransform>;
  layoutLockedRef: RefObject<boolean>;
  /** Rebind once the canvas element exists (first ResizeObserver tick). */
  enabled?: boolean;
  onSelectNode: (nodeId: string) => void;
  onNodeMove: (nodeId: string, position: Position) => void;
  /** Fires on pointerup when the pointer did not move — open the node editor. */
  onNodeClick?: (nodeId: string) => void;
  onDragActiveChange?: (active: boolean) => void;
};

/**
 * Pointer-capture node dragging for the storyboard canvas.
 *
 * OpenCanvas pans on pointer events; this hook must start on pointerdown in
 * the capture phase so a node click cannot become a pan, and it must not
 * rebind window listeners while React state updates (that dropped mouseup
 * and left the board unable to drag until a blank-canvas click).
 */
export function useShotNodeDrag({
  containerRef,
  nodesRef,
  viewportRef,
  layoutLockedRef,
  enabled = true,
  onSelectNode,
  onNodeMove,
  onNodeClick,
  onDragActiveChange,
}: UseShotNodeDragOptions) {
  const dragRef = useRef<DragSession | null>(null);
  const nodeDraggingRef = useRef(false);
  const onSelectNodeRef = useRef(onSelectNode);
  const onNodeMoveRef = useRef(onNodeMove);
  const onNodeClickRef = useRef(onNodeClick);
  const onDragActiveChangeRef = useRef(onDragActiveChange);
  onSelectNodeRef.current = onSelectNode;
  onNodeMoveRef.current = onNodeMove;
  onNodeClickRef.current = onNodeClick;
  onDragActiveChangeRef.current = onDragActiveChange;

  useEffect(() => {
    const container = containerRef.current;
    if (!enabled || !container) return;

    const finish = (event?: PointerEvent | Event) => {
      const drag = dragRef.current;
      if (!drag) return;
      if (event instanceof PointerEvent && event.pointerId !== drag.pointerId) return;

      const viewport = viewportRef.current;
      if (drag.moved && drag.canMove && viewport) {
        const point = event instanceof PointerEvent ? event : null;
        const dx = ((point?.clientX ?? drag.startX) - drag.startX) / Math.max(viewport.k, 0.05);
        const dy = ((point?.clientY ?? drag.startY) - drag.startY) / Math.max(viewport.k, 0.05);
        onNodeMoveRef.current(drag.id, { x: drag.origin.x + dx, y: drag.origin.y + dy });
      } else if (
        !drag.moved &&
        event instanceof PointerEvent &&
        event.type !== 'pointercancel'
      ) {
        onNodeClickRef.current?.(drag.id);
      }
      applyCanvasNodeDragPreview(container, null);
      dragRef.current = null;
      if (nodeDraggingRef.current) {
        nodeDraggingRef.current = false;
        onDragActiveChangeRef.current?.(false);
      }
      if (container.hasPointerCapture(drag.pointerId)) {
        try {
          container.releasePointerCapture(drag.pointerId);
        } catch {
          /* capture already released with the pointer */
        }
      }
    };

    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0) return;
      const target = event.target instanceof Element ? event.target : null;
      if (!target || target.closest(IGNORE_SELECTOR)) return;
      const nodeEl = target.closest<HTMLElement>('[data-node-id]');
      if (!nodeEl || !container.contains(nodeEl)) return;
      const nodeId = nodeEl.dataset.nodeId;
      if (!nodeId) return;
      const node = nodesRef.current.find((item) => item.id === nodeId);
      if (!node) return;

      event.stopPropagation();
      onSelectNodeRef.current(nodeId);

      dragRef.current = {
        id: nodeId,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        origin: { ...node.position },
        moved: false,
        canMove: !layoutLockedRef.current && !node.metadata?.locked,
      };
      try {
        container.setPointerCapture(event.pointerId);
      } catch {
        /* capture is optional; window listeners still receive the pointer */
      }
    };

    const onPointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      if (event.buttons === 0) {
        finish(event);
        return;
      }
      const viewport = viewportRef.current;
      if (!viewport) return;
      if (
        !drag.moved &&
        (Math.abs(event.clientX - drag.startX) > DRAG_THRESHOLD_PX ||
          Math.abs(event.clientY - drag.startY) > DRAG_THRESHOLD_PX)
      ) {
        drag.moved = true;
        nodeDraggingRef.current = true;
        onDragActiveChangeRef.current?.(true);
      }
      if (!drag.moved || !drag.canMove) return;
      event.preventDefault();
      const dx = (event.clientX - drag.startX) / Math.max(viewport.k, 0.05);
      const dy = (event.clientY - drag.startY) / Math.max(viewport.k, 0.05);
      applyCanvasNodeDragPreview(container, { x: dx, y: dy, nodeIds: new Set([drag.id]) });
    };

    const onPointerUp = (event: PointerEvent) => finish(event);
    const onBlur = () => finish();

    container.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    window.addEventListener('pointercancel', onPointerUp);
    window.addEventListener('blur', onBlur);
    return () => {
      applyCanvasNodeDragPreview(container, null);
      container.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
      window.removeEventListener('pointercancel', onPointerUp);
      window.removeEventListener('blur', onBlur);
    };
  }, [containerRef, enabled, layoutLockedRef, nodesRef, viewportRef]);

  return { nodeDraggingRef };
}
