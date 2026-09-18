import { useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  CanvasOverlay,
  overlayPanelStyle,
  useAnchoredOverlay,
} from '@oc/components/canvas/canvas-overlay';
import {
  anchoredOverlayStyle,
  type OverlayPlacement,
} from '@oc/lib/canvas/canvas-overlay';
import { useQuietChromeTheme } from '../quietChrome';

const OVERLAY_Z = 1200;

export function ComposerAnchoredOverlay({
  open,
  onOpenChange,
  trigger,
  children,
  width,
  placement = 'bottomLeft',
  estimatedHeight = 280,
  padded = true,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  trigger: ReactNode;
  children: ReactNode;
  width: number;
  placement?: OverlayPlacement;
  estimatedHeight?: number;
  padded?: boolean;
}) {
  const theme = useQuietChromeTheme();
  const triggerRef = useRef<HTMLSpanElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const rect = useAnchoredOverlay(open, triggerRef, panelRef, () => onOpenChange(false));
  const geometry = rect
    ? anchoredOverlayStyle(
        rect,
        { width: window.innerWidth, height: window.innerHeight },
        { width, placement, estimatedHeight }
      )
    : null;

  return (
    <>
      <span ref={triggerRef} className='inline-flex max-w-full'>
        {trigger}
      </span>
      {open && geometry
        ? createPortal(
            <CanvasOverlay
              ref={panelRef}
              theme={theme}
              style={{
                ...overlayPanelStyle(theme, geometry),
                zIndex: OVERLAY_Z,
                ...(padded ? null : { padding: 8 }),
              }}
              onPointerDown={(event) => event.stopPropagation()}
            >
              {children}
            </CanvasOverlay>,
            document.body
          )
        : null}
    </>
  );
}
