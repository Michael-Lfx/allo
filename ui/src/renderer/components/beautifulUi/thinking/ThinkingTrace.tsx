import { ChevronRight, Code2, Search, Sparkle } from 'lucide-react';
import React, { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isLiveProcessThinkingWindow, isThinkingTraceSettled } from './thinkingTraceModel';
import styles from './thinkingTrace.module.css';

export type ThinkingTraceVariant = 'steps' | 'reasoning' | 'search' | 'coding';
export type ThinkingTraceStatus = 'thinking' | 'waiting' | 'done' | 'failed' | 'canceled';
export type ThinkingTraceItemState = 'pending' | 'running' | 'done';

export type ThinkingTraceItem = {
  id: string;
  title: string;
  detail?: string;
  state?: ThinkingTraceItemState;
};

export type ThinkingTraceLayout = 'standalone' | 'process';

export type ThinkingTraceProps = {
  variant: ThinkingTraceVariant;
  status: ThinkingTraceStatus;
  items: ThinkingTraceItem[];
  elapsedSeconds?: number;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  /** Override the header label. Preview uses i18n defaults when omitted. */
  label?: string;
  layout?: ThinkingTraceLayout;
  bodyRef?: React.Ref<HTMLDivElement>;
  showElapsed?: boolean;
};

const processHeaderVariant = (variant: ThinkingTraceVariant): ThinkingTraceVariant => {
  switch (variant) {
    case 'search':
    case 'coding':
      return variant;
    case 'steps':
    case 'reasoning':
      return 'steps';
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

const variantIcon = (variant: ThinkingTraceVariant, size = 14): React.ReactNode => {
  const props = { size, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (variant) {
    case 'steps':
    case 'reasoning':
      return <Sparkle size={size} strokeWidth={0} fill='currentColor' aria-hidden />;
    case 'search':
      return <Search {...props} />;
    case 'coding':
      return <Code2 {...props} />;
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

const stopInnerWheelFromReachingTheList = (event: React.WheelEvent<HTMLDivElement>) => {
  event.stopPropagation();
};

const itemStateClass = (state: ThinkingTraceItemState | undefined): string => {
  switch (state) {
    case 'running':
      return styles.itemRunning;
    case 'done':
      return styles.itemDone;
    case 'pending':
    case undefined:
      return '';
    default: {
      const exhaustive: never = state;
      return exhaustive;
    }
  }
};

const ThinkingTrace: React.FC<ThinkingTraceProps> = ({
  variant,
  status,
  items,
  elapsedSeconds = 0,
  expanded,
  onExpandedChange,
  label,
  layout = 'standalone',
  bodyRef,
  showElapsed,
}) => {
  const { t } = useTranslation();
  const isSettled = isThinkingTraceSettled(status);
  const isProcess = layout === 'process';
  const liveWindow = isLiveProcessThinkingWindow(layout, status);
  const [internalExpanded, setInternalExpanded] = useState(() => expanded ?? !isSettled);
  const resolvedExpanded = liveWindow ? true : (expanded ?? internalExpanded);
  const bodyId = useId();
  const sUnit = t('common.unit.second_short', { defaultValue: 's' });
  const shouldShowElapsed = showElapsed ?? label === undefined;

  const handleToggle = () => {
    const nextExpanded = !resolvedExpanded;
    if (expanded === undefined) {
      setInternalExpanded(nextExpanded);
    }
    onExpandedChange?.(nextExpanded);
  };

  const defaultSummary = (): string => {
    switch (status) {
      case 'done':
        return t('beautifulUiPreview.thoughtComplete');
      case 'waiting':
        return t('beautifulUiPreview.waiting');
      case 'failed':
        return t('beautifulUiPreview.failed');
      case 'canceled':
        return t('beautifulUiPreview.canceled');
      case 'thinking':
        return t('beautifulUiPreview.thinking');
      default: {
        const exhaustive: never = status;
        return exhaustive;
      }
    }
  };

  const summary = label ?? defaultSummary();
  const statusClass = (): string => {
    switch (status) {
      case 'waiting':
        return ` ${styles.rootWaiting}`;
      case 'failed':
        return ` ${styles.rootFailed}`;
      case 'canceled':
        return ` ${styles.rootCanceled}`;
      case 'thinking':
      case 'done':
        return '';
      default: {
        const exhaustive: never = status;
        return exhaustive;
      }
    }
  };

  return (
    <div
      className={`${styles.root}${isProcess ? ` ${styles.rootProcess}` : ''}${variant === 'coding' ? ` ${styles.coding}` : ''}${statusClass()}`}
      data-testid='beautiful-ui-thinking'
      data-variant={variant}
      data-status={status}
      data-live-window={liveWindow ? 'true' : 'false'}
    >
      <button
        type='button'
        className={`${styles.header}${isProcess ? ` ${styles.headerProcess}` : ''}`}
        onClick={handleToggle}
        aria-expanded={resolvedExpanded}
        aria-controls={bodyId}
      >
        <span className={styles.icon} aria-hidden='true'>
          {variantIcon(isProcess ? processHeaderVariant(variant) : variant)}
        </span>
        <span className={`${styles.label} ${status === 'thinking' ? styles.shimmer : ''}`} title={summary}>
          {summary}
        </span>
        {!isSettled && shouldShowElapsed ? (
          <span className={styles.elapsed}>
            {Number.isInteger(elapsedSeconds) ? String(elapsedSeconds) : elapsedSeconds.toFixed(1)}
            {sUnit}
          </span>
        ) : null}
        <span className={`${styles.arrow} ${resolvedExpanded ? styles.arrowExpanded : ''}`} aria-hidden='true'>
          <ChevronRight size={12} strokeWidth={1.75} />
        </span>
      </button>
      <div
        ref={bodyRef}
        id={bodyId}
        className={`${styles.body}${isProcess ? ` ${styles.bodyProcess}` : ''} ${resolvedExpanded ? '' : styles.collapsed}`}
        onWheel={liveWindow ? stopInnerWheelFromReachingTheList : undefined}
      >
        {items.length === 0 && !liveWindow ? (
          label === undefined ? <div className={styles.empty}>{t('beautifulUiPreview.emptyTrace')}</div> : null
        ) : (
          <ol className={styles.list}>
            {items.map((item) =>
              item.title || item.detail ? (
                <li key={item.id} className={`${styles.item} ${itemStateClass(item.state)}`}>
                  {item.title ? <div className={styles.title}>{item.title}</div> : null}
                  {item.detail ? <div className={styles.detail}>{item.detail}</div> : null}
                </li>
              ) : null
            )}
          </ol>
        )}
      </div>
    </div>
  );
};

export default ThinkingTrace;
