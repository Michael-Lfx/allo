import { afterEach, describe, expect, test } from 'bun:test';

import { listenTauriEvent, tauriEvents } from './tauriEvents';

const originalWindow = (globalThis as { window?: unknown }).window;

const restoreWindow = (): void => {
  if (originalWindow === undefined) {
    Reflect.deleteProperty(globalThis, 'window');
  } else {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
  }
};

interface FakeInternals {
  invoke: (command: string, args?: unknown) => Promise<unknown>;
  callbacks: Map<number, (event: unknown) => void>;
  calls: Array<{ command: string; args: unknown }>;
}

const installTauri = (
  invoke?: (command: string, args?: unknown) => Promise<unknown>
): FakeInternals => {
  const callbacks = new Map<number, (event: unknown) => void>();
  const calls: Array<{ command: string; args: unknown }> = [];
  let nextId = 1;
  const internals = {
    transformCallback: (callback: (event: unknown) => void) => {
      const id = nextId++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback: (id: number) => {
      callbacks.delete(id);
    },
    metadata: { currentWindow: { label: 'main' } },
    invoke: async (command: string, args?: unknown) => {
      calls.push({ command, args });
      if (invoke) return invoke(command, args);
      if (command === 'plugin:event|unlisten') return undefined;
      return 1;
    },
  };
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: internals,
      __TAURI_EVENT_PLUGIN_INTERNALS__: {
        unregisterListener: (event: string, eventId: number) => {
          calls.push({ command: 'unlisten', args: { event, eventId } });
        },
      },
    },
  });
  return { invoke: internals.invoke, callbacks, calls };
};

afterEach(restoreWindow);

describe('tauriEvents WebUI fallback', () => {
  test('listen returns a no-op unsubscribe and never imports Tauri', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const unlisten = await tauriEvents.listen('demo', () => {
      throw new Error('should not fire');
    });
    expect(typeof unlisten).toBe('function');
    unlisten();
  });

  test('emit and emitTo resolve without a Tauri bridge', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    expect(await tauriEvents.emit('demo', { value: 1 })).toBeUndefined();
    expect(await tauriEvents.emitTo('main', 'demo', { value: 1 })).toBeUndefined();
  });
});

describe('tauriEvents Tauri bridge', () => {
  test('event sends preserve bridge failures for protocol callers', async () => {
    const failure = new Error('event delivery failed');
    installTauri(async () => { throw failure; });

    expect(await tauriEvents.emit('demo', {}).catch((error: unknown) => error)).toBe(failure);
    expect(await tauriEvents.emitTo('nomi-memory-panel', 'demo', {}).catch((error: unknown) => error)).toBe(failure);
  });

  test('listen forwards the payload and unsubscribes', async () => {
    const internals = installTauri();
    const payloads: number[] = [];
    const unlisten = await tauriEvents.listen<number>('demo', (payload) => payloads.push(payload));

    const callback = internals.callbacks.values().next().value;
    callback?.({ event: 'demo', id: 1, payload: 42 });
    expect(payloads).toEqual([42]);

    unlisten();
    await Promise.resolve();
    expect(internals.calls.some((call) => call.command === 'unlisten')).toBe(true);
  });

  test('listenCurrentWindow forwards the current-window payload', async () => {
    const internals = installTauri();
    const payloads: string[] = [];
    await tauriEvents.listenCurrentWindow<string>('demo', (payload) => payloads.push(payload));

    const callback = internals.callbacks.values().next().value;
    callback?.({ event: 'demo', id: 1, payload: 'hello' });
    expect(payloads).toEqual(['hello']);
  });

  test('emit and emitTo use the event bridge', async () => {
    const internals = installTauri();
    await tauriEvents.emit('demo', { value: 1 });
    await tauriEvents.emitTo('nomi-memory-panel', 'demo', { value: 2 });

    expect(internals.calls.map((call) => call.command)).toContain('plugin:event|emit');
    expect(internals.calls.map((call) => call.command)).toContain('plugin:event|emit_to');
  });

  test('emitTo validates an empty window label before touching Tauri', async () => {
    const internals = installTauri();
    let error: unknown;
    try {
      await tauriEvents.emitTo('  ', 'demo', {});
    } catch (caught) {
      error = caught;
    }
    expect(error instanceof Error).toBe(true);
    expect((error as Error).message).toContain('window label');
    expect(internals.calls).toHaveLength(0);
  });

  test('listenTauriEvent cancels a listener that resolves after disposal', async () => {
    let resolveListen: ((value: unknown) => void) | undefined;
    const internals = installTauri();
    // Replace the invoke with one that stays pending until the listener is cancelled.
    const pending = new Promise<unknown>((resolve) => {
      resolveListen = resolve;
    });
    const internalsWindow = (globalThis as unknown as { window: { __TAURI_INTERNALS__: { invoke: unknown } } }).window;
    internalsWindow.__TAURI_INTERNALS__.invoke = async (command: string, args?: unknown) => {
      internals.calls.push({ command, args });
      if (command === 'plugin:event|listen') return pending;
      return undefined;
    };

    const unlisten = listenTauriEvent('demo', () => {});
    unlisten();
    resolveListen?.(1);
    await pending;
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(internals.calls.some((call) => call.command === 'unlisten')).toBe(true);
  });
});
