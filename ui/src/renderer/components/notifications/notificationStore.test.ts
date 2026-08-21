import { describe, expect, test } from 'bun:test';
import { notificationStore, NOTIFICATION_EXIT_DURATION } from './notificationStore';
import type { NotificationScope } from './notificationTypes';

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const activeOf = (scope: NotificationScope) =>
  notificationStore.getSnapshot().filter((record) => record.scopeId === scope.id && record.status === 'active');

const anyOf = (scope: NotificationScope) => notificationStore.getSnapshot().filter((record) => record.scopeId === scope.id);

describe('notificationStore', () => {
  test('show applies Arco-compatible defaults (3000ms, icon for non-normal, scope closable)', () => {
    const scope = notificationStore.createScope();
    try {
      scope.show({ content: 'hello' });
      const [record] = activeOf(scope);
      expect(record.duration).toBe(3000);
      expect(record.level).toBe('info');
      expect(record.showIcon).toBe(true);
      expect(record.closable).toBe(false);
      expect(record.status).toBe('active');

      scope.show({ content: 'plain', level: 'normal' });
      const normal = activeOf(scope).find((item) => item.content === 'plain');
      expect(normal?.showIcon).toBe(false);
    } finally {
      scope.dispose();
    }
  });

  test('transient notifications expire and fire onClose exactly once', async () => {
    const scope = notificationStore.createScope();
    let closed = 0;
    try {
      scope.show({ content: 'bye', duration: 30, onClose: () => (closed += 1) });
      expect(activeOf(scope)).toHaveLength(1);
      await wait(30 + NOTIFICATION_EXIT_DURATION + 50);
      expect(anyOf(scope)).toHaveLength(0);
      expect(closed).toBe(1);
    } finally {
      scope.dispose();
    }
  });

  test('duration 0 persists; dismiss goes through exiting before removal', async () => {
    const scope = notificationStore.createScope();
    let closed = 0;
    try {
      const handle = scope.show({ id: 'sticky', content: 'working', duration: 0, onClose: () => (closed += 1) });
      await wait(60);
      expect(activeOf(scope)).toHaveLength(1);
      expect(closed).toBe(0);

      handle.dismiss();
      const exiting = anyOf(scope);
      expect(exiting).toHaveLength(1);
      expect(exiting[0].status).toBe('exiting');

      await wait(NOTIFICATION_EXIT_DURATION + 50);
      expect(anyOf(scope)).toHaveLength(0);
      expect(closed).toBe(1);
    } finally {
      scope.dispose();
    }
  });

  test('same id updates in place: content swapped, revision bumped, timer reset', async () => {
    const scope = notificationStore.createScope();
    try {
      scope.show({ id: 'task', content: 'loading', level: 'loading', duration: 0 });
      scope.show({ id: 'task', content: 'done', level: 'success' });
      const records = activeOf(scope);
      expect(records).toHaveLength(1);
      expect(records[0].content).toBe('done');
      expect(records[0].level).toBe('success');
      expect(records[0].revision).toBe(1);
      // Update without explicit duration falls back to the scope default,
      // so the former persistent record now auto-closes.
      expect(records[0].duration).toBe(3000);
    } finally {
      scope.dispose();
    }
  });

  test('handle.update patches content and duration on the same record', async () => {
    const scope = notificationStore.createScope();
    try {
      const handle = scope.show({ content: 'v1', duration: 0 });
      handle.update({ content: 'v2', duration: 30 });
      const [record] = activeOf(scope);
      expect(record.content).toBe('v2');
      expect(record.duration).toBe(30);
      await wait(30 + NOTIFICATION_EXIT_DURATION + 50);
      expect(anyOf(scope)).toHaveLength(0);
    } finally {
      scope.dispose();
    }
  });

  test('maxCount evicts the oldest active record of the scope', async () => {
    const scope = notificationStore.createScope({ maxCount: 2 });
    try {
      scope.show({ id: 'a', content: 'a', duration: 0 });
      scope.show({ id: 'b', content: 'b', duration: 0 });
      scope.show({ id: 'c', content: 'c', duration: 0 });
      await wait(NOTIFICATION_EXIT_DURATION + 50);
      expect(activeOf(scope).map((record) => record.id)).toEqual(['b', 'c']);
    } finally {
      scope.dispose();
    }
  });

  test('dismiss accepts both the internal key and the public id', async () => {
    const scope = notificationStore.createScope();
    try {
      scope.show({ id: 'by-id', content: 'x', duration: 0 });
      const handle = scope.show({ content: 'y', duration: 0 });
      scope.dismiss('by-id');
      scope.dismiss(handle.id); // handle.id is the internal key
      await wait(NOTIFICATION_EXIT_DURATION + 50);
      expect(anyOf(scope)).toHaveLength(0);
    } finally {
      scope.dispose();
    }
  });

  test('clear only removes records of its own scope', async () => {
    const scopeA = notificationStore.createScope();
    const scopeB = notificationStore.createScope();
    try {
      scopeA.show({ content: 'a', duration: 0 });
      scopeB.show({ content: 'b', duration: 0 });
      scopeA.clear();
      await wait(NOTIFICATION_EXIT_DURATION + 50);
      expect(anyOf(scopeA)).toHaveLength(0);
      expect(activeOf(scopeB)).toHaveLength(1);
    } finally {
      scopeA.dispose();
      scopeB.dispose();
    }
  });

  test('pauseInteraction freezes timers; resumeInteraction continues with the remainder', async () => {
    const scope = notificationStore.createScope();
    try {
      scope.show({ content: 'timed', duration: 60 });
      notificationStore.pauseInteraction('test-pause');
      await wait(120);
      expect(activeOf(scope)).toHaveLength(1);
      notificationStore.resumeInteraction('test-pause');
      await wait(60 + NOTIFICATION_EXIT_DURATION + 60);
      expect(anyOf(scope)).toHaveLength(0);
    } finally {
      notificationStore.resumeInteraction('test-pause');
      scope.dispose();
    }
  });

  test('scope dispose clears its records without firing their onClose twice', async () => {
    const scope = notificationStore.createScope();
    let closed = 0;
    scope.show({ content: 'x', duration: 0, onClose: () => (closed += 1) });
    scope.dispose();
    await wait(NOTIFICATION_EXIT_DURATION + 50);
    expect(anyOf(scope)).toHaveLength(0);
    expect(closed).toBe(1);
  });
});
