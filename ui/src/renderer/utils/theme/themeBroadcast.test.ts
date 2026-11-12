import { expect, test } from 'bun:test';
import { broadcastCustomCssSync, broadcastThemeSync, broadcastWallpaperSync } from './themeBroadcast';

test('theme broadcasts contain native delivery failures at the caller', async () => {
  const originalWindow = globalThis.window;
  const commands: string[] = [];
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: {
        invoke: async (command: string) => {
          commands.push(command);
          throw new Error('theme broadcast unavailable');
        },
      },
    },
  });

  try {
    broadcastThemeSync('dark');
    broadcastCustomCssSync('');
    broadcastWallpaperSync({ wallpaperTokens: '', wallpaperEnabled: false });
    // Allow the fire-and-forget sends (and their rejection handlers) to settle.
    // The Tauri event module is loaded with a dynamic import, which can take
    // more than one macrotask on a cold cache, so poll instead of a single tick.
    for (let attempt = 0; attempt < 50 && commands.length < 3; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(commands).toEqual(Array(3).fill('plugin:event|emit'));
  } finally {
    if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
    else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
  }
});
