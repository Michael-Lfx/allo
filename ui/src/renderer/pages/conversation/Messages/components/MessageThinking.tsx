
import type { IMessageThinking } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import ThinkingTrace from '@renderer/components/beautifulUi/thinking/ThinkingTrace';
import type { ThinkingTraceStatus, ThinkingTraceVariant } from '@renderer/components/beautifulUi/thinking/ThinkingTrace';
import {
  buildThinkingTraceItems,
  inferThinkingTraceVariant,
  isLiveProcessThinkingWindow,
  isThinkingTraceSettled,
  pinScrollableToLatest,
  resolveThinkingTraceStatus,
  type ThinkingTraceProcessState,
} from '@renderer/components/beautifulUi/thinking/thinkingTraceModel';
import type { TFunction } from 'i18next';
import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './MessageThinking.module.css';

interface MessageThinkingProps {
  message: IMessageThinking;
  variant?: 'standalone' | 'process';
  completed?: boolean;
  forceDone?: boolean;
  processState?: ThinkingTraceProcessState;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
}

const liveLabelForVariant = (variant: ThinkingTraceVariant, t: TFunction): string => {
  switch (variant) {
    case 'search':
      return t('conversation.thinking.search', { defaultValue: 'Searching...' });
    case 'coding':
      return t('conversation.thinking.coding', { defaultValue: 'Coding...' });
    case 'steps':
    case 'reasoning':
      return t('conversation.thinking.label', { defaultValue: 'Thinking...' });
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

const completeLabelForVariant = (variant: ThinkingTraceVariant, t: TFunction): string => {
  switch (variant) {
    case 'search':
      return t('conversation.thinking.searchComplete', { defaultValue: 'Search complete' });
    case 'coding':
      return t('conversation.thinking.codingComplete', { defaultValue: 'Code complete' });
    case 'steps':
    case 'reasoning':
      return t('conversation.thinking.complete', { defaultValue: 'Thought complete' });
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

const headerLabelForStatus = (
  status: ThinkingTraceStatus,
  variant: ThinkingTraceVariant,
  subject: string | undefined,
  t: TFunction
): string => {
  switch (status) {
    case 'waiting':
      return t('conversation.thinking.waiting', { defaultValue: 'Waiting for model output' });
    case 'failed':
      return t('conversation.thinking.failed', { defaultValue: 'Thinking failed' });
    case 'canceled':
      return t('conversation.thinking.canceled', { defaultValue: 'Thinking canceled' });
    case 'done':
      return completeLabelForVariant(variant, t);
    case 'thinking':
      return toDisplayText(subject) || liveLabelForVariant(variant, t);
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const MessageThinking: React.FC<MessageThinkingProps> = ({
  message,
  variant = 'standalone',
  completed,
  forceDone,
  processState,
  expanded,
  onExpandedChange,
}) => {
  const { t } = useTranslation();
  const isProcessVariant = variant === 'process';
  const { status, subject } = message.content;
  const text = toDisplayText(message.content.content);
  const traceStatus = resolveThinkingTraceStatus({
    messageStatus: status,
    completed,
    forceDone,
    processState,
  });
  const traceVariant = inferThinkingTraceVariant(text, toDisplayText(subject));
  const isDone = isThinkingTraceSettled(traceStatus);
  const liveWindow = isLiveProcessThinkingWindow(variant, traceStatus);
  const defaultExpanded = expanded ?? (isProcessVariant ? !isDone : true);
  const [internalExpanded, setInternalExpanded] = useState(() => defaultExpanded);
  const resolvedExpanded = liveWindow ? true : (expanded ?? (isProcessVariant ? !isDone : internalExpanded));
  const [elapsedTime, setElapsedTime] = useState(() => {
    const initialStartedAt = message.created_at ?? Date.now();
    return isDone ? 0 : Math.max(0, Math.floor((Date.now() - initialStartedAt) / 1000));
  });
  const startTimeRef = useRef<number>(message.created_at ?? Date.now());
  const bodyRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (expanded !== undefined) return;
    setInternalExpanded(defaultExpanded);
  }, [defaultExpanded, expanded, message.id, message.msg_id]);

  useEffect(() => {
    if (isDone) return;

    startTimeRef.current = message.created_at ?? Date.now();
    setElapsedTime(Math.max(0, Math.floor((Date.now() - startTimeRef.current) / 1000)));
    const timer = setInterval(() => {
      setElapsedTime(Math.floor((Date.now() - startTimeRef.current) / 1000));
    }, 1000);

    return () => clearInterval(timer);
  }, [isDone, message.created_at, message.msg_id]);

  useLayoutEffect(() => {
    if (!liveWindow || !resolvedExpanded) return;
    const element = bodyRef.current;
    if (!element) return;
    pinScrollableToLatest(element);
  }, [liveWindow, resolvedExpanded, text]);

  const handleToggle = (nextExpanded: boolean) => {
    if (expanded === undefined) {
      setInternalExpanded(nextExpanded);
    }
    onExpandedChange?.(nextExpanded);
  };

  const summaryText = headerLabelForStatus(traceStatus, traceVariant, subject, t);

  const items = useMemo(
    () => buildThinkingTraceItems(text, traceStatus),
    [text, traceStatus]
  );

  return (
    <div className={`${styles.container} ${isProcessVariant ? styles.containerProcess : ''}`}>
      <ThinkingTrace
        variant={traceVariant}
        status={traceStatus}
        items={items}
        label={summaryText}
        layout={variant}
        expanded={resolvedExpanded}
        onExpandedChange={handleToggle}
        bodyRef={bodyRef}
        elapsedSeconds={elapsedTime}
        showElapsed={!isDone}
      />
    </div>
  );
};

export default MessageThinking;
