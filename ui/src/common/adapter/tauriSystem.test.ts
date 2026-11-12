import { afterEach, describe, expect, test } from 'bun:test';

import { tauriSystem } from './tauriSystem';

const originalWindow = (globalThis as { window?: unknown }).window;

const restoreWindow = (): void => {
  if (originalWindow === undefined) Reflect.deleteProperty(globalThis, 'window');
  else Object.defineProperty(globalThis, 'window', { configurable: true, value: originalWindow });
};

afterEach(restoreWindow);

const expectResolved = async <T>(promise: Promise<T>): Promise<T> => promise;

describe('tauriSystem WebUI fallback', () => {
  test('system capabilities resolve without a Tauri bridge', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });

    expect(await expectResolved(tauriSystem.relaunch())).toBeUndefined();
    expect(await expectResolved(tauriSystem.openSupportLogsDir())).toBeUndefined();
    expect(await expectResolved(tauriSystem.getPath('home'))).toBe('');
    expect(tauriSystem.getZoom()).toBe(1);
    expect(await expectResolved(tauriSystem.setZoom(1.25))).toBe(1.25);
    expect(await expectResolved(tauriSystem.setKeepAwake(true))).toBeUndefined();
    expect(await expectResolved(tauriSystem.openDialog())).toBeUndefined();
    expect(await expectResolved(tauriSystem.saveDialog())).toBeNull();
    expect(await expectResolved(tauriSystem.isAutostartEnabled())).toBe(false);
    expect(await expectResolved(tauriSystem.setAutostart(true))).toBeUndefined();
    expect(await expectResolved(tauriSystem.sendNotification({ title: 't', body: 'b' }))).toBeUndefined();
    expect(await expectResolved(tauriSystem.clearAttention('a'))).toBeUndefined();
    expect(await expectResolved(tauriSystem.clearAttentionScope('conversation'))).toBeUndefined();
    expect(await expectResolved(tauriSystem.clearAllAttention())).toBeUndefined();
    expect(await expectResolved(tauriSystem.popupContextMenu([]))).toBeUndefined();
    expect(await expectResolved(tauriSystem.webviewLabel())).toBe('browser');
  });

  test('probe reports desktop_only and typed commands return undefined', async () => {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });

    expect(await expectResolved(tauriSystem.probeBackendLoopback())).toEqual({
      port: 0,
      tcp_connect: false,
      http_status: null,
      error: 'desktop_only',
    });
    expect(await expectResolved(tauriSystem.syncCompanionWindows([]))).toBeUndefined();
    expect(await expectResolved(tauriSystem.prepareCompanionMemoryPanel())).toBeUndefined();
    expect(await expectResolved(tauriSystem.showCompanionMemoryPanel({}))).toBeUndefined();
    expect(await expectResolved(tauriSystem.webui.getStatus())).toBeUndefined();
    expect(await expectResolved(tauriSystem.webui.start())).toBeUndefined();
    expect(await expectResolved(tauriSystem.webui.stop())).toBeUndefined();
  });
});