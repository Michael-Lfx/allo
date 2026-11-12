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
    // Allow the fire-and-forget sends and their rejection handlers to settle.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(commands).toEqual(Array(3).fill('plugin:event|emit'));
  } finally {
    if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
    else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
  }
});
