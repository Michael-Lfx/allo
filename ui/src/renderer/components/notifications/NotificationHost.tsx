import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import NotificationStackView from './NotificationStackView';
import type { NotificationStackViewLabels } from './NotificationStackView';
import { useNotificationBottomInset } from './notificationInsets';
import {
  getCollapsedRecords,
  getCollapseExitKeys,
  mergeCollapsedRecordsWithExits,
  sortByCreatedAt,
  textFromNode,
  MAX_VISIBLE_TRANSIENT,
} from './notificationStackModel';
import {
  announcementChannelForLevel,
  NotificationAnnouncementQueue,
  type NotificationAnnouncement,
} from './notificationAnnouncementQueue';
import { notificationStore } from './notificationStore';
import './notifications.css';

export const NOTIFICATION_COLLAPSE_EXIT_MS = 120;
export const NOTIFICATION_ANNOUNCEMENT_DISPLAY_MS = 1200;

const NotificationHost: React.FC = () => {
  const { t } = useTranslation();
  const records = useSyncExternalStore(notificationStore.subscribe, notificationStore.getSnapshot, notificationStore.getSnapshot);
  const bottomInset = useNotificationBottomInset();
  const [expanded, setExpanded] = useState(false);
  const [collapseExitKeys, setCollapseExitKeys] = useState<ReadonlySet<string>>(new Set());
  const [politeMessage, setPoliteMessage] = useState('');
  const [assertiveMessage, setAssertiveMessage] = useState('');
  const rootRef = useRef<HTMLDivElement>(null);
  const counterRef = useRef<HTMLButtonElement>(null);
  const cardsScrollRef = useRef<HTMLDivElement>(null);
  const announcedRef = useRef(new Map<string, number>());
  const announcementQueueRef = useRef(new NotificationAnnouncementQueue());
  const announcementInitializedRef = useRef(false);
  const activeAnnouncementRef = useRef<NotificationAnnouncement | null>(null);
  const announcementTimerRef = useRef<number | null>(null);
  const announcementFollowupTimerRef = useRef<number | null>(null);
  const flushAnnouncementRef = useRef<() => void>(() => undefined);
  const revealedRef = useRef(new Set<string>());
  const cardRefs = useRef(new Map<string, HTMLDivElement>());
  const previousRects = useRef(new Map<string, DOMRect>());
  const collapseExitTimerRef = useRef<number | null>(null);

  // Exiting records still render (120ms exit animation) but must not feed
  // counts, auto-collapse, or announcements — those read active-only.
  const activeRecords = useMemo(() => records.filter((notice) => notice.status === 'active'), [records]);
  const collapsed = useMemo(() => getCollapsedRecords(records), [records]);
  const collapsedKeys = useMemo(() => new Set(collapsed.records.map((notice) => notice.key)), [collapsed]);
  const effectiveCollapseExitKeys = useMemo(
    () => new Set([...collapseExitKeys].filter((key) => !collapsedKeys.has(key))),
    [collapseExitKeys, collapsedKeys],
  );
  const displayedRecords = expanded
    ? sortByCreatedAt(records)
    : mergeCollapsedRecordsWithExits(records, effectiveCollapseExitKeys);
  const shouldScroll = expanded || collapsed.scrollable;

  const labels = useMemo<NotificationStackViewLabels>(
    () => ({
      close: t('notifications.close', { defaultValue: '关闭通知' }),
      collapse: t('notifications.collapse', { defaultValue: '收起通知' }),
      more: (count) => t('notifications.more', { count, defaultValue: '还有 {{count}} 条通知' }),
    }),
    [t],
  );

  const clearCollapseExit = useCallback(() => {
    if (collapseExitTimerRef.current !== null) {
      window.clearTimeout(collapseExitTimerRef.current);
      collapseExitTimerRef.current = null;
    }
    setCollapseExitKeys((current) => (current.size === 0 ? current : new Set()));
  }, []);

  const requestCollapse = useCallback(() => {
    if (!expanded) return;

    const previousRecords = sortByCreatedAt(records);
    const nextCollapsedRecords = getCollapsedRecords(records).records;
    const exitKeys = getCollapseExitKeys(previousRecords, nextCollapsedRecords);
    const reducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

    if (collapseExitTimerRef.current !== null) window.clearTimeout(collapseExitTimerRef.current);
    if (exitKeys.length > 0 && !reducedMotion) {
      setCollapseExitKeys(new Set(exitKeys));
      collapseExitTimerRef.current = window.setTimeout(() => {
        collapseExitTimerRef.current = null;
        setCollapseExitKeys(new Set());
      }, NOTIFICATION_COLLAPSE_EXIT_MS + 72);
    } else {
      setCollapseExitKeys(new Set());
      collapseExitTimerRef.current = null;
    }
    setExpanded(false);
  }, [expanded, records]);

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
    if (!expanded) return;
    if (activeRecords.length === 0 || (collapsed.hiddenCount === 0 && activeRecords.length <= MAX_VISIBLE_TRANSIENT)) {
      requestCollapse();
    }
  }, [activeRecords.length, collapsed.hiddenCount, expanded, requestCollapse]);

  useEffect(
    () => () => {
      if (collapseExitTimerRef.current !== null) window.clearTimeout(collapseExitTimerRef.current);
    },
    [],
  );

  useEffect(() => {
    if (!expanded) return undefined;
    notificationStore.pauseInteraction('notification-expanded');
    return () => notificationStore.resumeInteraction('notification-expanded');
  }, [expanded]);

  flushAnnouncementRef.current = () => {
    if (activeAnnouncementRef.current) return;
    const next = announcementQueueRef.current.take();
    if (!next) return;

    activeAnnouncementRef.current = next;
    const setMessage = next.channel === 'assertive' ? setAssertiveMessage : setPoliteMessage;
    setMessage(next.message);
    announcementTimerRef.current = window.setTimeout(() => {
      activeAnnouncementRef.current = null;
      announcementTimerRef.current = null;
      setMessage('');
      announcementFollowupTimerRef.current = window.setTimeout(() => {
        announcementFollowupTimerRef.current = null;
        flushAnnouncementRef.current();
      }, 0);
    }, NOTIFICATION_ANNOUNCEMENT_DISPLAY_MS);
  };

  useEffect(() => {
    const sortedActiveRecords = sortByCreatedAt(activeRecords);
    const activeKeys = new Set(sortedActiveRecords.map((notice) => notice.key));
    announcementQueueRef.current.retain(activeKeys);
    announcedRef.current.forEach((_revision, key) => {
      if (!records.some((notice) => notice.key === key)) announcedRef.current.delete(key);
    });
    const enqueue = (notice: (typeof sortedActiveRecords)[number]) => {
      const fallback = t(`notifications.level.${notice.level}`, { defaultValue: notice.level });
      const message = textFromNode(notice.announce ?? notice.title) || textFromNode(notice.content) || fallback;
      const announcement = {
        key: notice.key,
        revision: notice.revision,
        createdAt: notice.createdAt,
        channel: announcementChannelForLevel(notice.level),
        message,
      } satisfies NotificationAnnouncement;
      announcedRef.current.set(notice.key, notice.revision);
      announcementQueueRef.current.enqueue(announcement);
    };

    if (!announcementInitializedRef.current) {
      announcementInitializedRef.current = true;
      sortedActiveRecords.forEach((notice) => announcedRef.current.set(notice.key, notice.revision));
      const newest = sortedActiveRecords[sortedActiveRecords.length - 1];
      if (newest) enqueue(newest);
    } else {
      sortedActiveRecords.forEach((notice) => {
        if (announcedRef.current.get(notice.key) !== notice.revision) enqueue(notice);
      });
    }

    flushAnnouncementRef.current();
  }, [activeRecords, records, t]);

  useEffect(
    () => () => {
      announcementQueueRef.current.clear();
      if (announcementTimerRef.current !== null) window.clearTimeout(announcementTimerRef.current);
      if (announcementFollowupTimerRef.current !== null) window.clearTimeout(announcementFollowupTimerRef.current);
      activeAnnouncementRef.current = null;
    },
    [],
  );

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
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) requestCollapse();
    };
    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => document.removeEventListener('pointerdown', handlePointerDown, true);
  }, [expanded, requestCollapse]);

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
      event.preventDefault();
      event.stopPropagation();
      requestCollapse();
      window.requestAnimationFrame(() => counterRef.current?.focus());
    }
  };

  const handleToggleExpanded = () => {
    if (expanded) {
      requestCollapse();
      return;
    }
    clearCollapseExit();
    setExpanded(true);
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
      collapsingKeys={expanded ? undefined : effectiveCollapseExitKeys}
      onToggleExpanded={handleToggleExpanded}
      onDismiss={(notice) => notificationStore.dismiss(notice.scopeId, notice.key)}
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      onFocusCapture={handleFocusCapture}
      onBlurCapture={handleBlurCapture}
      onKeyDown={handleKeyDown}
      rootRef={rootRef}
      counterRef={counterRef}
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
