import type { AppNotificationLevel } from './notificationTypes';

export type NotificationAnnouncementChannel = 'polite' | 'assertive';

export type NotificationAnnouncement = {
  key: string;
  revision: number;
  createdAt: number;
  channel: NotificationAnnouncementChannel;
  message: string;
};

export const announcementChannelForLevel = (level: AppNotificationLevel): NotificationAnnouncementChannel =>
  level === 'error' ? 'assertive' : 'polite';

/**
 * Keeps one pending announcement per notification. An update replaces a
 * pending older revision, while an announcement already being spoken is not
 * interrupted and can be followed by the newest revision.
 */
export class NotificationAnnouncementQueue {
  private readonly pending = new Map<string, NotificationAnnouncement>();

  enqueue(announcement: NotificationAnnouncement): void {
    const previous = this.pending.get(announcement.key);
    if (previous && previous.revision >= announcement.revision) return;
    this.pending.set(announcement.key, announcement);
  }

  has(channel?: NotificationAnnouncementChannel): boolean {
    if (!channel) return this.pending.size > 0;
    return [...this.pending.values()].some((announcement) => announcement.channel === channel);
  }

  take(channel?: NotificationAnnouncementChannel): NotificationAnnouncement | undefined {
    const next = [...this.pending.values()]
      .filter((announcement) => channel === undefined || announcement.channel === channel)
      .sort(
        (left, right) => left.createdAt - right.createdAt || left.key.localeCompare(right.key),
      )[0];
    if (next) this.pending.delete(next.key);
    return next;
  }

  retain(keys: ReadonlySet<string>): void {
    for (const key of this.pending.keys()) {
      if (!keys.has(key)) this.pending.delete(key);
    }
  }

  clear(): void {
    this.pending.clear();
  }
}
