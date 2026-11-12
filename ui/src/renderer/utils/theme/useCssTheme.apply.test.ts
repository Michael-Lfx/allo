/**
 * Behavioural tests for the serialized `applyCssThemeRaw` write path.
 *
 * Boundary: this file patches `configService` directly and does not mount the
 * full settings UI. It verifies the contract consumed by that UI: writes finish
 * before notification, failures broadcast the actually persisted CSS, the
 * original error is preserved, calls stay serialized, and a failed apply does
 * not wedge the queue.
 */

import { afterEach, describe, expect, test } from 'bun:test';
import { configService } from '@/common/config/configService';
import { applyCssThemeRaw } from '@/renderer/hooks/ui/useCssTheme';
import { subscribeThemeCustomCssUpdated } from './themeEvents';

type ConfigServiceDouble = {
  set: (key: string, value: unknown) => Promise<void>;
  get: (key: string) => unknown;
};

const service = configService as unknown as ConfigServiceDouble;
const originalSet = service.set;
const originalGet = service.get;
const cleanups: Array<() => void> = [];

const originalReportError = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
const originalConsoleError = console.error;

function installReportError(report: (error: unknown) => void): void {
  Object.defineProperty(globalThis, 'reportError', { configurable: true, writable: true, value: report });
}

afterEach(() => {
  while (cleanups.length > 0) {
    cleanups.pop()?.();
  }
  service.set = originalSet;
  service.get = originalGet;
  console.error = originalConsoleError;
  if (originalReportError) {
    Object.defineProperty(globalThis, 'reportError', originalReportError);
  } else {
    Reflect.deleteProperty(globalThis, 'reportError');
  }
});

describe('applyCssThemeRaw', () => {
  test('notifies only after both config writes complete', async () => {
    const events: string[] = [];
    const persisted: Record<string, unknown> = {};

    service.set = async (key, value) => {
      await Promise.resolve();
      persisted[key] = value;
      events.push(`write:${key}:${String(value)}`);
    };
    service.get = (key) => persisted[key];

    cleanups.push(
      subscribeThemeCustomCssUpdated(({ customCss }) => {
        events.push(`notify:${customCss}`);
      })
    );

    await applyCssThemeRaw('body { color: red; }', 'theme-1');

    expect(events).toEqual([
      'write:customCss:body { color: red; }',
      'write:css.activeThemeId:theme-1',
      'notify:body { color: red; }',
    ]);
  });

  test('broadcasts the persisted CSS and preserves the original error on failure', async () => {
    console.error = () => undefined;
    const failure = new Error('write rejected');
    const notifications: string[] = [];
    const persisted: Record<string, unknown> = { customCss: 'persisted-css' };

    service.set = async (key, value) => {
      if (key === 'customCss') throw failure;
      persisted[key] = value;
    };
    service.get = (key) => persisted[key];

    cleanups.push(
      subscribeThemeCustomCssUpdated(({ customCss }) => {
        notifications.push(customCss);
      })
    );

    let caught: unknown;
    try {
      await applyCssThemeRaw('new-css', 'theme-2');
    } catch (error) {
      caught = error;
    }

    expect(caught).toBe(failure);
    expect(notifications).toEqual(['persisted-css']);
  });

  test('serializes consecutive applies in call order', async () => {
    const events: string[] = [];
    let callIndex = 0;
    let releaseFirst: () => void = () => undefined;

    service.set = async (_key, value) => {
      callIndex += 1;
      events.push(`start:${String(value)}`);
      if (callIndex === 1) {
        await new Promise<void>((resolve) => {
          releaseFirst = resolve;
        });
      }
      events.push(`done:${String(value)}`);
    };
    service.get = () => '';

    cleanups.push(
      subscribeThemeCustomCssUpdated(({ customCss }) => {
        events.push(`notify:${customCss}`);
      })
    );

    const first = applyCssThemeRaw('first', 'theme-1');
    const second = applyCssThemeRaw('second', 'theme-2');

    await Promise.resolve();
    await Promise.resolve();
    expect(events.some((event) => event === 'start:second')).toBe(false);

    releaseFirst();
    await first;
    await second;

    expect(events).toEqual([
      'start:first',
      'start:theme-1',
      'done:theme-1',
      'done:first',
      'notify:first',
      'start:second',
      'done:second',
      'start:theme-2',
      'done:theme-2',
      'notify:second',
    ]);
  });

  test('the queue keeps processing after a rejected apply', async () => {
    console.error = () => undefined;
    const failure = new Error('first apply failed');
    const notifications: string[] = [];

    service.set = async (key, value) => {
      if (key === 'customCss' && value === 'bad') throw failure;
    };
    service.get = (key) => (key === 'customCss' ? 'safe-css' : '');

    cleanups.push(
      subscribeThemeCustomCssUpdated(({ customCss }) => {
        notifications.push(customCss);
      })
    );

    let caught: unknown;
    try {
      await applyCssThemeRaw('bad', 'theme-bad');
    } catch (error) {
      caught = error;
    }
    expect(caught).toBe(failure);

    await applyCssThemeRaw('good', 'theme-good');

    expect(notifications).toEqual(['safe-css', 'good']);
  });

  test('a throwing theme listener does not turn a successful write into a failure', async () => {
    const reported: unknown[] = [];
    installReportError((error) => reported.push(error));
    const notifications: string[] = [];
    const listenerFailure = new Error('listener exploded');

    service.set = async () => undefined;
    service.get = () => '';

    cleanups.push(
      subscribeThemeCustomCssUpdated(() => {
        throw listenerFailure;
      })
    );
    cleanups.push(
      subscribeThemeCustomCssUpdated(({ customCss }) => {
        notifications.push(customCss);
      })
    );

    const result = await applyCssThemeRaw('body {}', 'theme-3');
    expect(result).toBeUndefined();
    expect(reported).toEqual([listenerFailure]);
    expect(notifications).toEqual(['body {}']);
  });
});
