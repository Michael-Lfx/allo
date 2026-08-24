import { describe, expect, test } from 'bun:test';
import {
  announcementChannelForLevel,
  NotificationAnnouncementQueue,
  type NotificationAnnouncement,
} from './notificationAnnouncementQueue';

const announcement = (overrides: Partial<NotificationAnnouncement> = {}): NotificationAnnouncement => ({
  key: 'notice-1',
  revision: 1,
  createdAt: 1,
  channel: 'polite',
  message: '已保存',
  ...overrides,
});

describe('NotificationAnnouncementQueue', () => {
  test('routes errors to assertive and other levels to polite', () => {
    expect(announcementChannelForLevel('error')).toBe('assertive');
    expect(announcementChannelForLevel('success')).toBe('polite');
    expect(announcementChannelForLevel('loading')).toBe('polite');
  });

  test('keeps announcements ordered globally by creation time', () => {
    const queue = new NotificationAnnouncementQueue();
    queue.enqueue(announcement({ key: 'b', createdAt: 2, message: '第二条' }));
    queue.enqueue(announcement({ key: 'a', createdAt: 1, message: '第一条' }));

    expect(queue.take()?.message).toBe('第一条');
    expect(queue.take()?.message).toBe('第二条');
    expect(queue.take()).toBeUndefined();
  });

  test('replaces a pending notification with its newest revision', () => {
    const queue = new NotificationAnnouncementQueue();
    queue.enqueue(announcement({ revision: 1, message: '处理中' }));
    queue.enqueue(announcement({ revision: 2, message: '已完成' }));
    queue.enqueue(announcement({ revision: 1, message: '旧结果' }));

    expect(queue.take()).toMatchObject({ revision: 2, message: '已完成' });
  });

  test('preserves the channel on globally ordered announcements', () => {
    const queue = new NotificationAnnouncementQueue();
    queue.enqueue(announcement({ key: 'info', channel: 'polite', createdAt: 2, message: '提示' }));
    queue.enqueue(announcement({ key: 'error', channel: 'assertive', createdAt: 1, message: '失败' }));

    expect(queue.take()).toMatchObject({ channel: 'assertive', message: '失败' });
    expect(queue.take()).toMatchObject({ channel: 'polite', message: '提示' });
  });

  test('removes announcements that are no longer active', () => {
    const queue = new NotificationAnnouncementQueue();
    queue.enqueue(announcement({ key: 'stale' }));
    queue.enqueue(announcement({ key: 'live', createdAt: 2, message: '仍然存在' }));

    queue.retain(new Set(['live']));

    expect(queue.take()?.message).toBe('仍然存在');
    expect(queue.take()).toBeUndefined();
  });
});
