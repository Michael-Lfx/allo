import React from 'react';
import type { StoredNotification } from './notificationTypes';

export const MAX_VISIBLE_TRANSIENT = 3;
export const NOTIFICATION_STACK_ID = 'flowy-notification-stack';

export const textFromNode = (node: React.ReactNode): string => {
  if (node === null || node === undefined || typeof node === 'boolean') return '';
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(textFromNode).filter(Boolean).join(' ');
  if (React.isValidElement<{ children?: React.ReactNode }>(node)) return textFromNode(node.props.children);
  return '';
};

export const sortByCreatedAt = (records: readonly StoredNotification[]): StoredNotification[] =>
  records.slice().sort((left, right) => left.createdAt - right.createdAt);

/**
 * Collapsed-stack slot selection. Accepts the full record list (active +
 * exiting): an exiting record still occupies its slot for the 120ms exit
 * window so the stack does not jump, but it must not inflate `hiddenCount`
 * (the counter answers "how many live notifications are out of sight").
 */
export const getCollapsedRecords = (records: readonly StoredNotification[]) => {
  const sorted = sortByCreatedAt(records);
  const persistent = sorted.filter((notice) => notice.duration === 0);
  const transient = sorted.filter((notice) => notice.duration !== 0);
  const transientToShow =
    persistent.length >= MAX_VISIBLE_TRANSIENT
      ? []
      : transient.slice(-Math.max(0, MAX_VISIBLE_TRANSIENT - persistent.length));
  const visibleKeys = new Set([...persistent, ...transientToShow].map((notice) => notice.key));
  return {
    records: sorted.filter((notice) => visibleKeys.has(notice.key)),
    hiddenCount: transient.filter((notice) => !visibleKeys.has(notice.key) && notice.status === 'active').length,
    scrollable: persistent.length > MAX_VISIBLE_TRANSIENT,
  };
};
