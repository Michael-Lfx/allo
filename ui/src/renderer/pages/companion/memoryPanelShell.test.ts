import { afterEach, describe, expect, test } from 'bun:test';
import { emitToMemoryPanel, emitToWindow } from './memoryPanelShell';
import { MEMORY_PANEL_EVENTS, MEMORY_PANEL_LABEL } from './memoryPanelProtocol';

const originalWindow = globalThis.window;

afterEach(() => {
  if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
  else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
});

describe('memory panel event delivery', () => {
  test('a rejected present send reaches the owner failure handler', async () => {
    const failure = new Error('panel event delivery failed');
    const calls: Array<{ command: string; args: unknown }> = [];
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {
        __TAURI_INTERNALS__: {
          invoke: async (command: string, args: unknown) => {
            calls.push({ command, args });
            throw failure;
          },
        },
      },
    });

    const error = await emitToMemoryPanel(MEMORY_PANEL_EVENTS.present, {
      requestId: 'request-1', placement: 'above',
    }).catch((error: unknown) => error);
    expect(error).toBe(failure);
    expect(calls).toEqual([{
      command: 'plugin:event|emit_to',
      args: {
        target: { kind: 'AnyLabel', label: MEMORY_PANEL_LABEL },
        event: MEMORY_PANEL_EVENTS.present,
        payload: { requestId: 'request-1', placement: 'above' },
      },
    }]);
  });

  test('a rejected acknowledgement send reaches the panel caller', async () => {
    const failure = new Error('owner event delivery failed');
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { __TAURI_INTERNALS__: { invoke: async () => { throw failure; } } },
    });

    const error = await emitToWindow('companion-1', MEMORY_PANEL_EVENTS.closed, {
      requestId: 'request-1', reason: 'toggle', restoreFocus: false,
    }).catch((error: unknown) => error);
    expect(error).toBe(failure);
  });

  test('WebUI event sends remain no-ops', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
    expect(await emitToMemoryPanel(MEMORY_PANEL_EVENTS.present, {})).toBeUndefined();
    expect(await emitToWindow('companion-1', MEMORY_PANEL_EVENTS.closed, {})).toBeUndefined();
  });
});
