import { afterEach, describe, expect, test } from 'bun:test';

import { tauriWindows } from './tauriWindows';

const originalWindow = (globalThis as { window?: unknown }).window;
const originalDocument = (globalThis as { document?: unknown }).document;

const restoreGlobals = (): void => {
  if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
  else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
  if (originalDocument === undefined) Reflect.deleteProperty(globalThis, 'document');
  else Object.defineProperty(globalThis, 'document', { configurable: true, value: originalDocument });
};

const installTauri = (invoke: (command: string, args?: unknown) => Promise<unknown>): void => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: {
        invoke,
        transformCallback: () => 1,
        unregisterCallback: () => {},
        metadata: { currentWindow: { label: 'main' } },
      },
    },
  });
};

afterEach(restoreGlobals);

describe('tauriWindows WebUI fallback', () => {
  test('current handle is inert and keeps a stable label', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: { hasFocus: () => true },
    });
    const window = tauriWindows.current();
    expect(window.label).toBe('current');
    expect(await window.isMaximized()).toBe(false);
    expect(await window.outerPosition()).toEqual({ x: 0, y: 0 });
    expect(await window.outerSize()).toEqual({ width: 0, height: 0 });
    expect(typeof (await window.onMoved(() => {}))).toBe('function');
    expect(await tauriWindows.list()).toEqual([]);
    expect(await tauriWindows.monitors()).toEqual([]);
    expect(await tauriWindows.isAppFocused()).toBe(true);
  });
});

describe('tauriWindows Tauri bridge', () => {
  test('current window preserves native failures for boot retries and geometry reads', async () => {
    const failure = new Error('native window unavailable');
    installTauri(async () => { throw failure; });
    const window = tauriWindows.current();

    expect(await window.show().catch((error: unknown) => error)).toBe(failure);
    expect(await window.setPosition({ x: 10, y: 20 }).catch((error: unknown) => error)).toBe(failure);
    expect(await window.outerSize().catch((error: unknown) => error)).toBe(failure);
  });

  test('listed window handles preserve native mutation failures', async () => {
    const failure = new Error('capture failed');
    installTauri(async (command) => {
      if (command === 'plugin:window|get_all_windows') return ['main'];
      throw failure;
    });
    const [window] = await tauriWindows.list();

    expect(await window.setIgnoreCursorEvents(false).catch((error: unknown) => error)).toBe(failure);
  });

  test('a named handle retries resolution after a lookup failure', async () => {
    const failure = new Error('window enumeration unavailable');
    let lookups = 0;
    let shows = 0;
    installTauri(async (command) => {
      if (command === 'plugin:window|get_all_windows') {
        if (++lookups === 1) throw failure;
        return ['main', 'companion-1'];
      }
      if (command === 'plugin:window|show') shows += 1;
    });
    const window = tauriWindows.get('companion-1');

    expect(await window.show().catch((error: unknown) => error)).toBe(failure);
    await window.show();
    expect(lookups).toBe(2);
    expect(shows).toBe(1);
  });

  test('a missing named window rejects and can be resolved after it appears', async () => {
    let exists = false;
    installTauri(async (command) => {
      if (command === 'plugin:window|get_all_windows') return exists ? ['companion-1'] : [];
    });
    const window = tauriWindows.get('companion-1');

    const error = await window.show().catch((error: unknown) => error);
    expect(error instanceof Error).toBe(true);
    if (!(error instanceof Error)) throw new Error('missing window should reject');
    expect(error.message).toContain('companion-1');
    exists = true;
    await window.show();
  });

  test('list wraps native windows and exposes their labels', async () => {
    installTauri(async (command) => {
      if (command === 'plugin:window|get_all_windows') return ['main', 'companion-1'];
      if (command === 'plugin:window|is_focused') return false;
      return undefined;
    });

    const windows = await tauriWindows.list();
    expect(windows.map((window) => window.label)).toEqual(['main', 'companion-1']);
  });

  test('monitors normalize native monitor geometry', async () => {
    installTauri(async (command) => {
      if (command === 'plugin:window|available_monitors') {
        return [
          {
            name: 'Display 1',
            scaleFactor: 2,
            position: { x: 10, y: 20 },
            size: { width: 1920, height: 1080 },
            workArea: { position: { x: 10, y: 60 }, size: { width: 1920, height: 1040 } },
          },
        ];
      }
      return undefined;
    });

    expect(await tauriWindows.monitors()).toEqual([
      {
        name: 'Display 1',
        position: { x: 10, y: 20 },
        size: { width: 1920, height: 1080 },
        workArea: { position: { x: 10, y: 60 }, size: { width: 1920, height: 1040 } },
        scaleFactor: 2,
      },
    ]);
  });

  test('current window commands use the injected label', async () => {
    const calls: Array<{ command: string; args: unknown }> = [];
    installTauri(async (command, args) => {
      calls.push({ command, args });
      if (command === 'plugin:window|is_maximized') return true;
      if (command === 'plugin:window|outer_size') return { width: 320, height: 240 };
      return undefined;
    });

    const window = tauriWindows.current();
    await window.maximize();
    expect(await window.isMaximized()).toBe(true);
    expect(await window.outerSize()).toEqual({ width: 320, height: 240 });
    expect(calls.some((call) => call.command === 'plugin:window|maximize')).toBe(true);
    expect(calls.every((call) => (call.args as { label?: string } | undefined)?.label === 'main')).toBe(true);
  });

  test('isAppFocused enumerates windows and reports any focused sibling', async () => {
    installTauri(async (command, args) => {
      if (command === 'plugin:window|get_all_windows') return ['main', 'companion-1'];
      if (command === 'plugin:window|is_focused') {
        const { label } = args as { label: string };
        return label === 'companion-1';
      }
      return undefined;
    });

    expect(await tauriWindows.isAppFocused()).toBe(true);
  });

  test('isAppFocused falls back to the current window when enumeration fails', async () => {
    installTauri(async (command) => {
      if (command === 'plugin:window|get_all_windows') throw new Error('enumeration unavailable');
      if (command === 'plugin:window|is_focused') return true;
      return undefined;
    });

    expect(await tauriWindows.isAppFocused()).toBe(true);
  });

  test('get rejects an empty label before hitting Tauri', () => {
    installTauri(async () => undefined);
    expect(() => tauriWindows.get('  ')).toThrow('window label');
  });
});
