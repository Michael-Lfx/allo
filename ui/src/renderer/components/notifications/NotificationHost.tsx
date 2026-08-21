import React, { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import NotificationStackView from './NotificationStackView';
import type { NotificationStackViewLabels } from './NotificationStackView';
import { useNotificationBottomInset } from './notificationInsets';
import { getCollapsedRecords, sortByCreatedAt, textFromNode, MAX_VISIBLE_TRANSIENT } from './notificationStackModel';
import { notificationStore } from './notificationStore';
import './notifications.css';

const NotificationHost: React.FC = () => {
  const { t } = useTranslation();
  const records = useSyncExternalStore(notificationStore.subscribe, notificationStore.getSnapshot, notificationStore.getSnapshot);
  const bottomInset = useNotificationBottomInset();
  const [expanded, setExpanded] = useState(false);
  const [politeMessage, setPoliteMessage] = useState('');
  const [assertiveMessage, setAssertiveMessage] = useState('');
  const rootRef = useRef<HTMLDivElement>(null);
  const cardsScrollRef = useRef<HTMLDivElement>(null);
  const announcedRef = useRef(new Map<string, number>());
  const revealedRef = useRef(new Set<string>());
  const cardRefs = useRef(new Map<string, HTMLDivElement>());
  const previousRects = useRef(new Map<string, DOMRect>());

  // Exiting records still render (120ms exit animation) but must not feed
  // counts, auto-collapse, or announcements — those read active-only.
  const activeRecords = useMemo(() => records.filter((notice) => notice.status === 'active'), [records]);
  const collapsed = useMemo(() => getCollapsedRecords(records), [records]);
  const collapsedKeys = useMemo(() => new Set(collapsed.records.map((notice) => notice.key)), [collapsed]);
  const displayedRecords = expanded ? sortByCreatedAt(records) : collapsed.records;
  const shouldScroll = expanded || collapsed.scrollable;

  const labels = useMemo<NotificationStackViewLabels>(
    () => ({
      close: t('notifications.close', { defaultValue: '关闭通知' }),
      collapse: t('notifications.collapse', { defaultValue: '收起通知' }),
      more: (count) => t('notifications.more', { count, defaultValue: '还有 {{count}} 条通知' }),
      moreLabel: t('notifications.moreLabel', { defaultValue: '更多通知' }),
    }),
    [t],
  );

  // Cards hidden by the collapse limit get a one-time stagger when expansion
  // first reveals them; later expansions of the same cards enter without delay.
  const newlyRevealedKeys = useMemo(() => {
    if (!expanded) return undefined;
    const fresh = new Set<string>();
    records.forEach((notice) => {
      if (!collapsedKeys.has(notice.key) && !revealedRef.current.has(notice.key)) fresh.add(notice.key);
    });
    return fresh;
  }, [expanded, records, collapsedKeys]);

  useEffect(() => {
    const presentKeys = new Set(records.map((notice) => notice.key));
    revealedRef.current.forEach((key) => {
      if (!presentKeys.has(key)) revealedRef.current.delete(key);
    });
    if (!expanded) return;
    records.forEach((notice) => {
      if (!collapsedKeys.has(notice.key)) revealedRef.current.add(notice.key);
    });
  }, [expanded, records, collapsedKeys]);

  useEffect(() => {
    if (!expanded && collapsed.hiddenCount === 0) return;
    if (expanded && activeRecords.length === 0) setExpanded(false);
    if (expanded && collapsed.hiddenCount === 0 && activeRecords.length <= MAX_VISIBLE_TRANSIENT) {
      setExpanded(false);
    }
  }, [activeRecords.length, collapsed.hiddenCount, expanded]);

  useEffect(() => {
    if (!expanded) return undefined;
    notificationStore.pauseInteraction('notification-expanded');
    return () => notificationStore.resumeInteraction('notification-expanded');
  }, [expanded]);

  useEffect(() => {
    const changed = activeRecords
      .filter((notice) => announcedRef.current.get(notice.key) !== notice.revision)
      .sort((left, right) => right.createdAt - left.createdAt)[0];
    if (!changed) return;
    announcedRef.current.set(changed.key, changed.revision);
    const fallback = t(`notifications.level.${changed.level}`, { defaultValue: changed.level });
    const content = textFromNode(changed.announce ?? changed.title) || textFromNode(changed.content) || fallback;
    if (changed.level === 'error') {
      setAssertiveMessage(content);
      const timer = window.setTimeout(() => setAssertiveMessage(''), 1200);
      return () => window.clearTimeout(timer);
    }
    setPoliteMessage(content);
    const timer = window.setTimeout(() => setPoliteMessage(''), 1200);
    return () => window.clearTimeout(timer);
  }, [activeRecords, t]);

  useLayoutEffect(() => {
    if (cardsScrollRef.current && expanded) cardsScrollRef.current.scrollTop = cardsScrollRef.current.scrollHeight;
  }, [displayedRecords.length, expanded]);

  useLayoutEffect(() => {
    const nextRects = new Map<string, DOMRect>();
    const animations: Animation[] = [];
    displayedRecords.forEach((notice) => {
      const node = cardRefs.current.get(notice.key);
      if (!node) return;
      const next = node.getBoundingClientRect();
      nextRects.set(notice.key, next);
      const previous = previousRects.current.get(notice.key);
      if (!previous || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return;
      const deltaY = previous.top - next.top;
      if (Math.abs(deltaY) < 0.5) return;
      animations.push(
        node.animate([{ transform: `translateY(${deltaY}px)` }, { transform: 'translateY(0)' }], {
          duration: 240,
          easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
        }),
      );
    });
    previousRects.current = nextRects;
    return () => animations.forEach((animation) => animation.cancel());
  }, [displayedRecords]);

  useEffect(() => {
    if (!expanded) return undefined;
    const handlePointerDown = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setExpanded(false);
    };
    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => document.removeEventListener('pointerdown', handlePointerDown, true);
  }, [expanded]);

  const handlePointerEnter = () => notificationStore.pauseInteraction('notification-pointer');
  const handlePointerLeave = () => notificationStore.resumeInteraction('notification-pointer');
  const handleFocusCapture = () => notificationStore.pauseInteraction('notification-focus');
  const handleBlurCapture = (event: React.FocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      notificationStore.resumeInteraction('notification-focus');
    }
  };
  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape' && rootRef.current?.contains(document.activeElement)) {
      event.stopPropagation();
      setExpanded(false);
    }
  };

  if (typeof document === 'undefined') return null;

  return createPortal(
    <NotificationStackView
      displayedRecords={displayedRecords}
      expanded={expanded}
      hiddenCount={collapsed.hiddenCount}
      shouldScroll={shouldScroll}
      bottomInset={bottomInset}
      livePoliteMessage={politeMessage}
      liveAssertiveMessage={assertiveMessage}
      labels={labels}
      newlyRevealedKeys={newlyRevealedKeys}
      onToggleExpanded={() => setExpanded((value) => !value)}
      onDismiss={(notice) => notificationStore.dismiss(notice.scopeId, notice.key)}
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      onFocusCapture={handleFocusCapture}
      onBlurCapture={handleBlurCapture}
      onKeyDown={handleKeyDown}
      rootRef={rootRef}
      cardsScrollRef={cardsScrollRef}
      setCardRef={(key, node) => {
        if (node) cardRefs.current.set(key, node);
        else cardRefs.current.delete(key);
      }}
    />,
    document.body,
  );
};

export default NotificationHost;
