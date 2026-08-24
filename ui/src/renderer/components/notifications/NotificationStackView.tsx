import { Down } from '@icon-park/react';
import React from 'react';
import NotificationCard from './NotificationCard';
import { NOTIFICATION_STACK_ID } from './notificationStackModel';
import type { StoredNotification } from './notificationTypes';

export const NOTIFICATION_STAGGER_STEP_MS = 24;
export const NOTIFICATION_STAGGER_MAX_MS = 96;

export type NotificationStackViewLabels = {
  close: string;
  collapse: string;
  more: (count: number) => string;
  moreLabel: string;
};

export type NotificationStackViewProps = {
  displayedRecords: readonly StoredNotification[];
  expanded: boolean;
  hiddenCount: number;
  shouldScroll: boolean;
  bottomInset: number;
  livePoliteMessage: string;
  liveAssertiveMessage: string;
  labels: NotificationStackViewLabels;
  newlyRevealedKeys?: ReadonlySet<string>;
  onToggleExpanded: () => void;
  onDismiss: (notice: StoredNotification) => void;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
  onFocusCapture: React.FocusEventHandler<HTMLDivElement>;
  onBlurCapture: React.FocusEventHandler<HTMLDivElement>;
  onKeyDown: React.KeyboardEventHandler<HTMLDivElement>;
  rootRef?: React.Ref<HTMLDivElement>;
  counterRef?: React.Ref<HTMLButtonElement>;
  cardsScrollRef?: React.Ref<HTMLDivElement>;
  setCardRef?: (key: string, node: HTMLDivElement | null) => void;
};

const NotificationStackView: React.FC<NotificationStackViewProps> = ({
  displayedRecords,
  expanded,
  hiddenCount,
  shouldScroll,
  bottomInset,
  livePoliteMessage,
  liveAssertiveMessage,
  labels,
  newlyRevealedKeys,
  onToggleExpanded,
  onDismiss,
  onPointerEnter,
  onPointerLeave,
  onFocusCapture,
  onBlurCapture,
  onKeyDown,
  rootRef,
  counterRef,
  cardsScrollRef,
  setCardRef,
}) => {
  let staggerIndex = 0;

  return (
    <div
      ref={rootRef}
      className='flowy-notification-host'
      style={{ '--flowy-notification-bottom-inset': `${bottomInset}px` } as React.CSSProperties}
      onKeyDown={onKeyDown}
      onFocusCapture={onFocusCapture}
      onBlurCapture={onBlurCapture}
    >
      <div
        className='flowy-notification-stack'
        data-expanded={expanded ? 'true' : 'false'}
        onPointerEnter={onPointerEnter}
        onPointerLeave={onPointerLeave}
      >
        {(expanded || hiddenCount > 0) && (
          <button
            type='button'
            ref={counterRef}
            className='flowy-notification-stack__counter'
            aria-expanded={expanded}
            aria-controls={NOTIFICATION_STACK_ID}
            aria-label={expanded ? labels.collapse : labels.more(hiddenCount)}
            onClick={onToggleExpanded}
          >
            {!expanded && <span className='flowy-notification-stack__counter-pill'>{hiddenCount}</span>}
            <span className='flowy-notification-stack__counter-text'>
              {expanded ? labels.collapse : labels.moreLabel}
            </span>
            <Down theme='outline' size='14' className='flowy-notification-stack__counter-chevron' aria-hidden='true' />
          </button>
        )}
        <div
          id={NOTIFICATION_STACK_ID}
          ref={cardsScrollRef}
          className={`flowy-notification-stack__cards ${shouldScroll ? 'flowy-notification-stack__cards--scrollable' : ''}`}
        >
          {displayedRecords.map((notice) => {
            let staggerStyle: React.CSSProperties | undefined;
            if (expanded && newlyRevealedKeys?.has(notice.key)) {
              const delay = Math.min(staggerIndex * NOTIFICATION_STAGGER_STEP_MS, NOTIFICATION_STAGGER_MAX_MS);
              staggerIndex += 1;
              if (delay > 0) staggerStyle = { animationDelay: `${delay}ms` };
            }
            return (
              <NotificationCard
                key={notice.key}
                notice={notice}
                closeLabel={labels.close}
                style={staggerStyle}
                onDismiss={() => onDismiss(notice)}
                cardRef={
                  setCardRef
                    ? (node) => {
                        setCardRef(notice.key, node);
                      }
                    : undefined
                }
              />
            );
          })}
        </div>
      </div>
      <div className='flowy-notification-live' role='alert' aria-live='assertive' aria-atomic='true'>
        {liveAssertiveMessage}
      </div>
      <div className='flowy-notification-live' role='status' aria-live='polite' aria-atomic='true'>
        {livePoliteMessage}
      </div>
    </div>
  );
};

export default NotificationStackView;
