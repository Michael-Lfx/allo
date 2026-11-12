import { afterEach, describe, expect, test } from 'bun:test';
import { emitter } from '@/renderer/utils/emitter';
import {
  dispatchThemeCssThemesChanged,
  dispatchThemeCustomCssUpdated,
  subscribeThemeCssThemesChanged,
  subscribeThemeCustomCssUpdated,
} from './themeEvents';

const cleanups: Array<() => void> = [];

function track<T extends () => void>(cleanup: T): T {
  cleanups.push(cleanup);
  return cleanup;
}

const originalReportError = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
const originalConsoleError = console.error;

function installReportError(report: (error: unknown) => void): void {
  Object.defineProperty(globalThis, 'reportError', { configurable: true, writable: true, value: report });
}

function restoreReportError(): void {
  if (originalReportError) {
    Object.defineProperty(globalThis, 'reportError', originalReportError);
    return;
  }
  Reflect.deleteProperty(globalThis, 'reportError');
}

afterEach(() => {
  while (cleanups.length > 0) {
    cleanups.pop()?.();
  }
  console.error = originalConsoleError;
  restoreReportError();
});

describe('theme custom CSS event', () => {
  test('delivers synchronously and in registration order, including empty CSS clears', () => {
    const deliveries: Array<[string, string]> = [];

    track(subscribeThemeCustomCssUpdated(({ customCss }) => deliveries.push(['first', customCss])));
    track(subscribeThemeCustomCssUpdated(({ customCss }) => deliveries.push(['second', customCss])));

    dispatchThemeCustomCssUpdated('body { color: red; }');
    expect(deliveries).toEqual([
      ['first', 'body { color: red; }'],
      ['second', 'body { color: red; }'],
    ]);

    deliveries.length = 0;
    dispatchThemeCustomCssUpdated('');
    expect(deliveries).toEqual([
      ['first', ''],
      ['second', ''],
    ]);
  });

  test('supports individual unsubscribe and leaves no listeners after repeated mount cleanup', () => {
    const first: string[] = [];
    const second: string[] = [];

    const unsubscribeFirst = subscribeThemeCustomCssUpdated(({ customCss }) => first.push(customCss));
    track(subscribeThemeCustomCssUpdated(({ customCss }) => second.push(customCss)));

    dispatchThemeCustomCssUpdated('one');
    unsubscribeFirst();
    dispatchThemeCustomCssUpdated('two');

    expect(first).toEqual(['one']);
    expect(second).toEqual(['one', 'two']);
    expect(emitter.listenerCount('theme.custom-css.updated')).toBe(1);

    const unsubscribeMounted = subscribeThemeCustomCssUpdated(() => undefined);
    const unsubscribeMountedAgain = subscribeThemeCustomCssUpdated(() => undefined);
    unsubscribeMounted();
    unsubscribeMountedAgain();
    expect(emitter.listenerCount('theme.custom-css.updated')).toBe(1);
  });

  test('reports a throwing listener without blocking later listeners or the sender', () => {
    const reported: unknown[] = [];
    installReportError((error) => reported.push(error));
    const failure = new Error('listener exploded');
    const received: string[] = [];

    track(
      subscribeThemeCustomCssUpdated(() => {
        throw failure;
      }),
    );
    track(subscribeThemeCustomCssUpdated(({ customCss }) => received.push(customCss)));

    expect(() => dispatchThemeCustomCssUpdated('after-failure')).not.toThrow();
    expect(reported).toEqual([failure]);
    expect(received).toEqual(['after-failure']);
  });

  test('falls back to console.error when reportError is unavailable', () => {
    Reflect.deleteProperty(globalThis, 'reportError');
    const logged: unknown[][] = [];
    console.error = (...args: unknown[]) => {
      logged.push(args);
    };
    const failure = new Error('listener exploded');
    const received: string[] = [];

    track(
      subscribeThemeCustomCssUpdated(() => {
        throw failure;
      }),
    );
    track(subscribeThemeCustomCssUpdated(({ customCss }) => received.push(customCss)));

    expect(() => dispatchThemeCustomCssUpdated('fallback')).not.toThrow();
    expect(logged).toHaveLength(1);
    expect(logged[0]?.includes(failure)).toBe(true);
    expect(received).toEqual(['fallback']);
  });
});

describe('theme list changed event', () => {
  test('notifies every subscriber synchronously and keeps failures isolated', () => {
    const reported: unknown[] = [];
    installReportError((error) => reported.push(error));
    const calls: string[] = [];
    const failure = new Error('theme picker exploded');

    track(
      subscribeThemeCssThemesChanged(() => {
        calls.push('first');
      }),
    );
    track(
      subscribeThemeCssThemesChanged(() => {
        throw failure;
      }),
    );
    track(
      subscribeThemeCssThemesChanged(() => {
        calls.push('third');
      }),
    );

    expect(() => dispatchThemeCssThemesChanged()).not.toThrow();
    expect(calls).toEqual(['first', 'third']);
    expect(reported).toEqual([failure]);
  });

  test('unsubscribes independently and cleans up every listener', () => {
    let first = 0;
    let second = 0;
    const unsubscribeFirst = subscribeThemeCssThemesChanged(() => {
      first += 1;
    });
    track(
      subscribeThemeCssThemesChanged(() => {
        second += 1;
      }),
    );

    dispatchThemeCssThemesChanged();
    unsubscribeFirst();
    dispatchThemeCssThemesChanged();

    expect(first).toBe(1);
    expect(second).toBe(2);
    expect(emitter.listenerCount('theme.css-themes.changed')).toBe(1);
  });
});
