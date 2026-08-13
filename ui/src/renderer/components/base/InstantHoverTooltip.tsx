

import classNames from 'classnames';
import { autoUpdate, flip, offset, shift, useFloating } from '@floating-ui/react';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

export type InstantHoverTooltipProps = {
  content: React.ReactNode;
  children: React.ReactNode;
  position?: 'top' | 'right' | 'bottom' | 'left';
  className?: string;
  hoverDelayMs?: number;
  dataTauriNoDrag?: boolean;
};

const GAP_PX = 6;
const VIEWPORT_PADDING_PX = 8;

const InstantHoverTooltip: React.FC<InstantHoverTooltipProps> = ({
  content,
  children,
  position = 'top',
  className,
  hoverDelayMs = 0,
  dataTauriNoDrag = false,
}) => {
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [visible, setVisible] = useState(false);

  const { refs, floatingStyles, isPositioned } = useFloating({
    open: visible,
    placement: position,
    strategy: 'fixed',
    middleware: [
      offset(GAP_PX),
      flip({ padding: VIEWPORT_PADDING_PX }),
      shift({ padding: VIEWPORT_PADDING_PX }),
    ],
    whileElementsMounted: autoUpdate,
  });

  const clearPendingShow = useCallback(() => {
    if (hoverTimerRef.current === null) return;
    clearTimeout(hoverTimerRef.current);
    hoverTimerRef.current = null;
  }, []);

  const showNow = useCallback(() => {
    clearPendingShow();
    setVisible(true);
  }, [clearPendingShow]);

  const showAfterHoverDelay = useCallback(() => {
    clearPendingShow();
    if (hoverDelayMs <= 0) {
      showNow();
      return;
    }
    hoverTimerRef.current = setTimeout(() => {
      hoverTimerRef.current = null;
      showNow();
    }, hoverDelayMs);
  }, [clearPendingShow, hoverDelayMs, showNow]);

  const hide = useCallback(() => {
    clearPendingShow();
    setVisible(false);
  }, [clearPendingShow]);

  useEffect(() => clearPendingShow, [clearPendingShow]);

  const tooltip =
    visible && typeof document !== 'undefined'
      ? createPortal(
          <span
            ref={refs.setFloating}
            role='tooltip'
            className='instant-hover-tooltip pointer-events-none fixed z-[10001] box-border whitespace-normal break-words rd-6px px-8px py-5px text-12px font-500 leading-none text-white shadow-[0_6px_18px_rgba(0,0,0,0.18)]'
            style={{
              ...floatingStyles,
              maxWidth: 'calc(100vw - 16px)',
              visibility: isPositioned ? 'visible' : 'hidden',
            }}
          >
            {content}
          </span>,
          document.body
        )
      : null;

  return (
    <>
      <div
        ref={refs.setReference}
        className={classNames('relative inline-flex shrink-0', className)}
        data-tauri-no-drag={dataTauriNoDrag || undefined}
        onMouseEnter={showAfterHoverDelay}
        onMouseLeave={hide}
        onFocus={showNow}
        onBlur={hide}
      >
        {children}
      </div>
      {tooltip}
    </>
  );
};

export default InstantHoverTooltip;
