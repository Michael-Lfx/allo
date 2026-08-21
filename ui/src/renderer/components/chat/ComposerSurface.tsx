import React, { type CSSProperties, type HTMLAttributes, useMemo } from 'react';
import { mergeRefs, useNotificationBlocker } from '@/renderer/components/notifications';
import './composerSurface.css';

export type ComposerSurfaceProps = {
  outerRef?: React.Ref<HTMLDivElement>;
  panelRef?: React.Ref<HTMLDivElement>;
  dragHandlers?: HTMLAttributes<HTMLDivElement>;
  dragHandlersTarget?: 'outer' | 'panel';
  isOverlayOpen?: boolean;
  overflowTarget?: 'outer' | 'panel';
  className?: string;
  panelClassName?: string;
  style?: CSSProperties;
  panelStyle?: CSSProperties;
  before?: React.ReactNode;
  beforePanel?: React.ReactNode;
  afterPanel?: React.ReactNode;
  children: React.ReactNode;
};

/** Shared DOM shell for the home and conversation composers. */
const ComposerSurface: React.FC<ComposerSurfaceProps> = ({
  outerRef,
  panelRef,
  dragHandlers,
  dragHandlersTarget = 'outer',
  isOverlayOpen = false,
  overflowTarget = 'outer',
  className,
  panelClassName,
  style,
  panelStyle,
  before,
  beforePanel,
  afterPanel,
  children,
}) => {
  const notificationBlockerRef = useNotificationBlocker();
  const composedOuterRef = useMemo(() => mergeRefs(notificationBlockerRef, outerRef), [notificationBlockerRef, outerRef]);
  const outerDragHandlers = dragHandlersTarget === 'outer' ? dragHandlers : undefined;
  const panelDragHandlers = dragHandlersTarget === 'panel' ? dragHandlers : undefined;
  const outerOverflowClass = overflowTarget === 'outer' ? (isOverlayOpen ? 'overflow-visible' : 'overflow-hidden') : 'overflow-visible';
  const panelOverflowClass = overflowTarget === 'panel' ? (isOverlayOpen ? 'overflow-visible' : 'overflow-hidden') : 'overflow-visible';

  return (
    <div
      ref={composedOuterRef}
      className={`composer-surface relative flex flex-col ${outerOverflowClass} ${className ?? ''}`}
      style={style}
      {...outerDragHandlers}
    >
      {before}
      <div
        ref={panelRef}
        className={`composer-surface__panel relative flex flex-col ${panelOverflowClass} ${panelClassName ?? ''}`}
        style={panelStyle}
        {...panelDragHandlers}
      >
        {beforePanel}
        {children}
      </div>
      {afterPanel}
    </div>
  );
};

export default ComposerSurface;
