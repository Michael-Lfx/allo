

import classNames from 'classnames';
import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

export type InstantHoverTooltipProps = {
  content: React.ReactNode;
  children: React.ReactNode;
  position?: 'top' | 'right' | 'bottom' | 'left';
  className?: string;
  hoverDelayMs?: number;
};

type TooltipCoords = {
  top: number;
  left: number;
};

const GAP_PX = 6;

const transformClassName: Record<NonNullable<InstantHoverTooltipProps['position']>, string> = {
  top: '-translate-x-1/2 -translate-y-full',
  right: '-translate-y-1/2',
  bottom: '-translate-x-1/2 translate-y-1px',
  left: '',
};

export function computeTooltipCoords(rect: DOMRect, position: NonNullable<InstantHoverTooltipProps['position']>): TooltipCoords {
  switch (position) {
    case 'top':
      return { top: rect.top - GAP_PX, left: rect.left + rect.width / 2 };
    case 'right':
      return { top: rect.top + rect.height / 2, left: rect.right + GAP_PX };
    case 'bottom':
      // 底部居中显示
      const centeredLeft = rect.left + rect.width / 2;
      return { top: rect.bottom + GAP_PX, left: centeredLeft };
    default:
      return { top: rect.top, left: rect.left };
  }
}

const InstantHoverTooltip: React.FC<InstantHoverTooltipProps> = ({
  content,
  children,
  position = 'top',
  className,
  hoverDelayMs = 0,
}) => {
  const anchorRef = useRef<HTMLDivElement>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [visible, setVisible] = useState(false);
  const [coords, setCoords] = useState<TooltipCoords | null>(null);

  const syncCoords = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    setCoords(computeTooltipCoords(anchor.getBoundingClientRect(), position));
  }, [position]);

  useLayoutEffect(() => {
    if (!visible) return undefined;
    syncCoords();

    const handleReposition = () => syncCoords();
    window.addEventListener('resize', handleReposition);
    window.addEventListener('scroll', handleReposition, true);
    return () => {
      window.removeEventListener('resize', handleReposition);
      window.removeEventListener('scroll', handleReposition, true);
    };
  }, [visible, syncCoords, content]);

  const clearPendingShow = useCallback(() => {
    if (hoverTimerRef.current === null) return;
    clearTimeout(hoverTimerRef.current);
    hoverTimerRef.current = null;
  }, []);

  const showNow = useCallback(() => {
    clearPendingShow();
    syncCoords();
    setVisible(true);
  }, [clearPendingShow, syncCoords]);

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
    visible && coords && typeof document !== 'undefined'
      ? createPortal(
          <span
            role='tooltip'
            className={classNames(
              'instant-hover-tooltip pointer-events-none fixed z-[10001] whitespace-nowrap rd-6px px-8px py-5px text-12px font-500 leading-none text-white shadow-[0_6px_18px_rgba(0,0,0,0.18)]',
              transformClassName[position]
            )}
            style={{
              top: coords.top,
              left: coords.left,
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
        ref={anchorRef}
        className={classNames('relative inline-flex shrink-0', className)}
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
