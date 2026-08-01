/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
import type { ConversationId } from '@/common/types/ids';

import React from 'react';
import { useMessageList } from '@renderer/pages/conversation/Messages/hooks';
import { dispatchChatMessageJump } from '@/renderer/utils/chat/chatMinimapEvents';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './ConversationQuestionLocator.module.css';
import { buildTurnPreview, truncate } from './minimapUtils';

type VirtuosoRange = { startIndex: number; endIndex: number };
type ConversationQuestionLocatorProps = {
  conversation_id?: ConversationId;
  /** Rendered window of the virtualized message list (default → all rendered). */
  rangeRef?: React.RefObject<VirtuosoRange>;
  /** Resolves a question message id → its row index in the message list. */
  resolveDisplayIndexRef?: React.RefObject<(messageId: string) => number | null>;
};

export const pickActiveQuestionIndex = (questionTopOffsets: number[], anchorY: number): number => {
  if (!questionTopOffsets.length) return -1;
  if (questionTopOffsets[0] > anchorY) return 0;

  let activeIndex = 0;
  for (let index = 0; index < questionTopOffsets.length; index += 1) {
    if (questionTopOffsets[index] <= anchorY) {
      activeIndex = index;
      continue;
    }
    break;
  }
  return activeIndex;
};

export const getDotDistanceLevel = (index: number, activeIndex: number): 0 | 1 | 2 | 3 => {
  const distance = Math.abs(index - activeIndex);
  if (distance <= 0) return 0;
  if (distance === 1) return 1;
  if (distance === 2) return 2;
  return 3;
};

const ConversationQuestionLocator: React.FC<ConversationQuestionLocatorProps> = ({
  conversation_id,
  rangeRef,
  resolveDisplayIndexRef,
}) => {
  const { t } = useTranslation();
  const list = useMessageList();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const rafRef = useRef<number | null>(null);
  const turns = useMemo(() => buildTurnPreview(list), [list]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const activeItem = turns[Math.min(activeIndex, turns.length - 1)];
  const previewIndex = Math.min(hoverIndex ?? activeIndex, turns.length - 1);
  const previewItem = turns[previewIndex];

  const getScroller = useCallback(() => {
    const host = rootRef.current?.parentElement;
    return host?.querySelector<HTMLElement>('[data-testid="message-list-scroller"]') ?? null;
  }, []);

  const syncActiveQuestionFromScroll = useCallback(() => {
    const scroller = getScroller();
    if (!scroller || !turns.length) return;

    const scrollerRect = scroller.getBoundingClientRect();
    const anchorY = Math.min(180, Math.max(96, scrollerRect.height * 0.34));
    // react-virtuoso unmounts off-screen messages, so a DOM probe alone misreads
    // them as "infinitely far below". Use the rendered window + display-index
    // resolver (passed in from MessageList) to bucket each question, and only
    // measure the ones that are actually mounted. Falls back to pure DOM probing
    // when no range is provided (non-virtualized / legacy callers).
    const range = rangeRef?.current ?? { startIndex: 0, endIndex: Number.POSITIVE_INFINITY };
    const resolveDisplayIndex = resolveDisplayIndexRef?.current;
    const measureTop = (businessId: string | undefined): number => {
      if (!businessId) return Number.POSITIVE_INFINITY;
      const el = document.querySelector<HTMLElement>(`[data-message-business-id="${businessId}"]`);
      if (!el) return Number.POSITIVE_INFINITY;
      return el.getBoundingClientRect().top - scrollerRect.top;
    };
    const questionTopOffsets = turns.map((item) => {
      const businessId = item.messageId ?? item.msgId;
      const displayIndex = businessId && resolveDisplayIndex ? resolveDisplayIndex(businessId) : null;
      if (displayIndex == null) return measureTop(businessId);
      if (displayIndex < range.startIndex) return Number.NEGATIVE_INFINITY; // scrolled past (above window)
      if (displayIndex > range.endIndex) return Number.POSITIVE_INFINITY; // not yet reached (below window)
      return measureTop(businessId); // inside rendered window → precise measurement
    });
    const nextIndex = pickActiveQuestionIndex(questionTopOffsets, anchorY);
    if (nextIndex >= 0) {
      setActiveIndex((current) => (current === nextIndex ? current : nextIndex));
    }
  }, [getScroller, rangeRef, resolveDisplayIndexRef, turns]);

  const scheduleActiveQuestionSync = useCallback(() => {
    if (rafRef.current !== null) return;
    rafRef.current = window.requestAnimationFrame(() => {
      rafRef.current = null;
      syncActiveQuestionFromScroll();
    });
  }, [syncActiveQuestionFromScroll]);

  useLayoutEffect(() => {
    setActiveIndex(0);
    setHoverIndex(null);
  }, [conversation_id]);

  useEffect(() => {
    const scroller = getScroller();
    if (!scroller || !turns.length) return;

    syncActiveQuestionFromScroll();
    scroller.addEventListener('scroll', scheduleActiveQuestionSync, { passive: true });
    window.addEventListener('resize', scheduleActiveQuestionSync);
    return () => {
      scroller.removeEventListener('scroll', scheduleActiveQuestionSync);
      window.removeEventListener('resize', scheduleActiveQuestionSync);
      if (rafRef.current !== null) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [getScroller, scheduleActiveQuestionSync, syncActiveQuestionFromScroll, turns.length]);

  const jumpToQuestion = useCallback((index: number) => {
    const item = turns[index];
    if (!conversation_id || !item) return;
    dispatchChatMessageJump({
      conversation_id,
      messageId: item.messageId,
      msgId: item.msgId,
      align: 'start',
      behavior: 'smooth',
    });
  }, [conversation_id, turns]);

  if (!conversation_id || !activeItem) return null;

  return (
    <div
      ref={rootRef}
      className={styles.root}
      data-testid='conversation-question-locator'
      data-tooltip-visible={hoverIndex !== null ? 'true' : undefined}
      onMouseLeave={() => setHoverIndex(null)}
    >
      <div
        className={styles.track}
        data-testid='conversation-question-locator-track'
        role='list'
        aria-label={t('conversation.minimap.locatorAria', { defaultValue: 'Open question history' })}
      >
        {turns.map((item, index) => {
          const isActive = activeIndex === index;
          const isHovered = hoverIndex === index;
          return (
            <button
              key={item.messageId || item.msgId || item.index}
              type='button'
              className={styles.dotButton}
              data-testid='conversation-question-locator-dot'
              data-active={isActive ? 'true' : undefined}
              data-hovered={isHovered ? 'true' : undefined}
              data-distance-level={getDotDistanceLevel(index, activeIndex)}
              aria-current={isActive ? 'true' : undefined}
              aria-label={t('conversation.minimap.locatorItemAria', {
                defaultValue: 'Jump to question {{index}}',
                index: item.index,
              })}
              title={truncate(item.questionRaw || item.question, 72)}
              onClick={() => jumpToQuestion(index)}
              onBlur={() => setHoverIndex((current) => (current === index ? null : current))}
              onFocus={() => setHoverIndex(index)}
              onMouseEnter={() => setHoverIndex(index)}
            >
              <span className={styles.dot} aria-hidden='true' />
            </button>
          );
        })}
      </div>
      <button
        type='button'
        className={styles.tooltipBubble}
        data-testid='conversation-question-locator-tooltip'
        onClick={() => jumpToQuestion(previewIndex)}
      >
        <span className={styles.tooltipTitle}>{truncate(previewItem.questionRaw || previewItem.question, 72)}</span>
        {previewItem.answer ? (
          <span className={styles.tooltipExcerpt}>{truncate(previewItem.answerRaw || previewItem.answer, 156)}</span>
        ) : null}
      </button>
    </div>
  );
};

export default ConversationQuestionLocator;
