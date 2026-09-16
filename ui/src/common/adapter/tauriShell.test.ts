/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import {
  tauriDownloadUpdate,
  tauriInstallUpdate,
  tauriIsAppFocused,
  tauriSendNotification,
  type TauriDownloadUpdateProgress,
} from './tauriShell';

const originalWindow = globalThis.window;

const restoreWindow = (): void => {
  if (typeof originalWindow === 'undefined') {
    Reflect.deleteProperty(globalThis, 'window');
  } else {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
  }
};

const withTauriInternals = async (
  invoke: (command: string, args: unknown, options: unknown) => Promise<unknown>,
  run: () => Promise<void>
): Promise<void> => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: {
        invoke,
        transformCallback: () => 1,
        unregisterCallback: () => {},
        // `getCurrentWindow()` resolves its label from this metadata.
        metadata: { currentWindow: { label: 'main' } },
      },
    },
  });

  try {
    await run();
  } finally {
    restoreWindow();
  }
};

const withDocumentFocus = async (
  hasFocus: boolean,
  run: () => Promise<void>
): Promise<void> => {
  const originalDocument = (globalThis as { document?: unknown }).document;
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: { hasFocus: () => hasFocus },
  });

  try {
    await run();
  } finally {
    if (originalDocument === undefined) {
      Reflect.deleteProperty(globalThis, 'document');
    } else {
      Object.defineProperty(globalThis, 'document', {
        configurable: true,
        value: originalDocument,
      });
    }
  }
};

const focusInvoke = (
  focusedLabels: string[],
  options: { throwOnEnumerate?: boolean; throwOnFocus?: boolean } = {}
) =>
  async (command: string, args: unknown): Promise<unknown> => {
    if (command === 'plugin:window|get_all_windows') {
      if (options.throwOnEnumerate) throw new Error('window enumeration unavailable');
      return ['main', 'nomi-memory-panel'];
    }
    if (command === 'plugin:window|is_focused') {
      if (options.throwOnFocus) throw new Error('focus query unavailable');
      const { label } = args as { label: string };
      return focusedLabels.includes(label);
    }
    throw new Error(`unexpected command: ${command}`);
  };

describe('native update commands', () => {
  test('download invokes the Rust-owned command and forwards progress', async () => {
    const calls: Array<{ command: string; args: unknown; options: unknown }> = [];
    const events: TauriDownloadUpdateProgress[] = [];
    await withTauriInternals(async (command, args, options) => {
      calls.push({ command, args, options });
      const payload = args as {
        onEvent: { onmessage: (event: TauriDownloadUpdateProgress) => void };
      };
      payload.onEvent.onmessage({ phase: 'downloading', chunkLength: 64, contentLength: 128 });
    }, async () => {
      await tauriDownloadUpdate('1.2.3', (event) => events.push(event));
    });

    expect(calls).toHaveLength(1);
    expect(calls[0]?.command).toBe('download_update');
    expect((calls[0]?.args as { version: string }).version).toBe('1.2.3');
    expect(calls[0]?.options).toBeUndefined();
    expect(events).toEqual([{ phase: 'downloading', chunkLength: 64, contentLength: 128 }]);
  });

  test('install invokes a separate command with no download channel', async () => {
    const calls: Array<{ command: string; args: unknown; options: unknown }> = [];
    await withTauriInternals(async (command, args, options) => {
      calls.push({ command, args, options });
    }, async () => {
      await tauriInstallUpdate('1.2.3');
    });

    expect(calls).toEqual([
      { command: 'install_update', args: { version: '1.2.3' }, options: undefined },
    ]);
  });

  test('propagates native installation failures', async () => {
    let errorMessage = '';
    await withTauriInternals(async () => {
      throw new Error('native updater failed');
    }, async () => {
      try {
        await tauriInstallUpdate('1.2.3');
      } catch (error) {
        errorMessage = error instanceof Error ? error.message : String(error);
      }
    });

    expect(errorMessage).toBe('native updater failed');
  });

  test('notification forwards the pending-attention id to Rust', async () => {
    const calls: Array<{ command: string; args: unknown }> = [];
    await withTauriInternals(async (command, args) => {
      calls.push({ command, args });
    }, async () => {
      await tauriSendNotification({
        title: '客服回复了你',
        body: '你好',
        attention_id: 'support:7',
        click_target: 'flowy://support?attention_id=support%3A7',
      });
    });

    expect(calls).toEqual([
      {
        command: 'show_os_notification_cmd',
        args: {
          title: '客服回复了你',
          body: '你好',
          clickTarget: 'flowy://support?attention_id=support%3A7',
          attentionId: 'support:7',
        },
      },
    ]);
  });
});

describe('app-level window focus', () => {
  test('is focused when any Flowy window holds OS focus', async () => {
    await withTauriInternals(focusInvoke(['nomi-memory-panel']), async () => {
      await withDocumentFocus(false, async () => {
        expect(await tauriIsAppFocused()).toBe(true);
      });
    });
  });

  test('is unfocused when no window holds OS focus', async () => {
    await withTauriInternals(focusInvoke([]), async () => {
      await withDocumentFocus(true, async () => {
        expect(await tauriIsAppFocused()).toBe(false);
      });
    });
  });

  test('falls back to the main window when enumeration fails', async () => {
    await withTauriInternals(
      focusInvoke(['main'], { throwOnEnumerate: true }),
      async () => {
        await withDocumentFocus(false, async () => {
          expect(await tauriIsAppFocused()).toBe(true);
        });
      }
    );
  });

  test('falls back to the DOM signal when every native query fails', async () => {
    await withTauriInternals(
      focusInvoke([], { throwOnEnumerate: true, throwOnFocus: true }),
      async () => {
        await withDocumentFocus(true, async () => {
          expect(await tauriIsAppFocused()).toBe(true);
        });
        await withDocumentFocus(false, async () => {
          expect(await tauriIsAppFocused()).toBe(false);
        });
      }
    );
  });
});
