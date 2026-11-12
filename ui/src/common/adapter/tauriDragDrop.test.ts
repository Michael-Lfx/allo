import { afterEach, describe, expect, test } from 'bun:test';

import { tauriDragDrop, type TauriDragDropEvent } from './tauriDragDrop';

const originalWindow = (globalThis as { window?: unknown }).window;

const restoreWindow = (): void => {
  if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
  else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
};

interface DragDropHarness {
  fire: (index: number, event: unknown) => void;
  calls: Array<{ command: string; args: unknown }>;
}

const installTauri = (): DragDropHarness => {
  const callbacks: Array<(event: unknown) => void> = [];
  const calls: Array<{ command: string; args: unknown }> = [];
  let nextId = 1;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: {
        invoke: async (command: string, args?: unknown) => {
          calls.push({ command, args });
          if (command === 'plugin:event|listen') return nextId++;
          return undefined;
        },
        transformCallback: (callback: (event: unknown) => void) => {
          callbacks.push(callback);
          return callbacks.length;
        },
        unregisterCallback: () => {},
        metadata: {
          currentWindow: { label: 'main' },
          currentWebview: { label: 'main' },
        },
      },
    },
  });
  return {
    calls,
    fire: (index, event) => callbacks[index]?.(event),
  };
};

afterEach(restoreWindow);

describe('tauriDragDrop adapter', () => {
  test('WebUI subscribe is a no-op', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    const unlisten = await tauriDragDrop.subscribe(() => {
      throw new Error('should not fire');
    });
    expect(typeof unlisten).toBe('function');
    unlisten();
  });

  test('normalizes native drop paths and preserves physical position', async () => {
    const harness = installTauri();
    const events: TauriDragDropEvent[] = [];
    await tauriDragDrop.subscribe((event) => events.push(event));

    harness.fire(2, {
      payload: {
        paths: ['file:///C:/Users/Alice/My%20File.png'],
        position: { x: 640, y: 480 },
      },
    });

    expect(events).toHaveLength(1);
    expect(events[0]?.type).toBe('drop');
    expect(events[0]?.paths).toEqual(['C:/Users/Alice/My File.png']);
    expect(events[0]?.position).toMatchObject({ x: 640, y: 480 });
  });

  test('maps enter and leave events without paths', async () => {
    const harness = installTauri();
    const events: TauriDragDropEvent[] = [];
    await tauriDragDrop.subscribe((event) => events.push(event));

    harness.fire(0, { payload: { paths: [], position: { x: 10, y: 20 } } });
    harness.fire(3, { payload: {} });

    expect(events.map((event) => event.type)).toEqual(['enter', 'leave']);
    expect(events[0]?.paths).toEqual([]);
    expect(events[0]?.position).toMatchObject({ x: 10, y: 20 });
    expect(events[1]?.position).toBeUndefined();
  });

  test('pathsToFileMetadata uses normalized path and basename', () => {
    const [file] = tauriDragDrop.pathsToFileMetadata(['file:///tmp/My%20File.PNG']);
    expect(file?.path).toBe('/tmp/My File.PNG');
    expect(file?.name).toBe('My File.PNG');
    expect(file?.type).toBe('image/png');
    expect(file?.size).toBe(0);
  });
});