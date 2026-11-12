/**
 * Unified system capability adapter.
 *
 * System paths, dialogs, notifications, autostart, keep-awake, zoom, restart,
 * diagnostics, native menus, and WebUI lifecycle live here. Existing
 * `tauriShell.ts` implementations remain the single source of truth; this
 * module reuses them instead of re-issuing the same Tauri command or plugin
 * call. Every method degrades safely in the WebUI host.
 */
import { isTauriRuntime } from './tauriRuntime';
import { tauriEvents } from './tauriEvents';
import {
  tauriClearAllAttention,
  tauriClearAttention,
  tauriClearAttentionScope,
  tauriGetPath,
  tauriGetZoom,
  tauriIsAutostartEnabled,
  tauriOpenDialog,
  tauriOpenSupportLogsDir,
  tauriProbeBackendLoopback,
  tauriRelaunch,
  tauriSaveDialog,
  tauriSendNotification,
  tauriSetAutostart,
  tauriSetKeepAwake,
  tauriSetTrayLabels,
  tauriSetZoom,
  tauriWebuiGetStatus,
  tauriWebuiStart,
  tauriWebuiStop,
  type AttentionSource,
  type BackendLoopbackProbe,
  type ShellOpenDialogOptions,
  type ShellSaveDialogOptions,
} from './tauriShell';

export type { AttentionSource, BackendLoopbackProbe, ShellOpenDialogOptions, ShellSaveDialogOptions };

export interface NotificationOptions {
  title: string;
  body: string;
  icon?: string;
  click_target?: string;
  attention_id?: string;
}

export interface ContextMenuEntry {
  id: string;
  text: string;
  action: () => void;
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) return undefined as T;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

const desktopOnlyProbe = (): BackendLoopbackProbe => ({
  port: 0,
  tcp_connect: false,
  http_status: null,
  error: 'desktop_only',
});

export const tauriSystem = {
  async relaunch(): Promise<void> {
    if (isTauriRuntime()) await tauriRelaunch();
  },

  async openSupportLogsDir(): Promise<void> {
    if (isTauriRuntime()) await tauriOpenSupportLogsDir();
  },

  async probeBackendLoopback(): Promise<BackendLoopbackProbe> {
    return isTauriRuntime() ? tauriProbeBackendLoopback() : desktopOnlyProbe();
  },

  async getPath(name: 'desktop' | 'home' | 'downloads'): Promise<string> {
    return isTauriRuntime() ? tauriGetPath(name) : '';
  },

  getZoom(): number {
    return tauriGetZoom();
  },

  async setZoom(factor: number): Promise<number> {
    return isTauriRuntime() ? tauriSetZoom(factor) : factor;
  },

  async setKeepAwake(enabled: boolean): Promise<void> {
    if (isTauriRuntime()) await tauriSetKeepAwake(enabled);
  },

  async setTrayLabels(
    show: string,
    quit: string,
    meeting?: {
      start?: string;
      pause?: string;
      resume?: string;
      stop?: string;
      open?: string;
    }
  ): Promise<void> {
    if (isTauriRuntime()) await tauriSetTrayLabels(show, quit, meeting);
  },

  async openDialog(options?: ShellOpenDialogOptions): Promise<string[] | undefined> {
    return isTauriRuntime() ? tauriOpenDialog(options) : undefined;
  },

  async saveDialog(options?: ShellSaveDialogOptions): Promise<string | null> {
    return isTauriRuntime() ? tauriSaveDialog(options) : null;
  },

  async isAutostartEnabled(): Promise<boolean> {
    return isTauriRuntime() ? tauriIsAutostartEnabled() : false;
  },

  async setAutostart(enabled: boolean): Promise<void> {
    if (isTauriRuntime()) await tauriSetAutostart(enabled);
  },

  async sendNotification(options: NotificationOptions): Promise<void> {
    if (isTauriRuntime()) await tauriSendNotification(options);
  },

  async clearAttention(attentionId: string): Promise<void> {
    if (isTauriRuntime()) await tauriClearAttention(attentionId);
  },

  async clearAttentionScope(source: AttentionSource, entityId?: string): Promise<void> {
    if (isTauriRuntime()) await tauriClearAttentionScope(source, entityId);
  },

  async clearAllAttention(): Promise<void> {
    if (isTauriRuntime()) await tauriClearAllAttention();
  },

  activateCompletionToast: (generation: number) =>
    invokeCommand<void>('activate_completion_toast', { generation }),

  dismissCompletionToast: (generation: number) =>
    invokeCommand<void>('dismiss_completion_toast', { generation }),

  syncCompanionWindows: (specs: Array<{ companion_id: string; enabled: boolean }>) =>
    invokeCommand<void>('sync_companion_windows', { specs }),

  getCompanionLocalPointer: <T>() => invokeCommand<T>('get_companion_local_pointer'),

  reportVoiceContext: (args: {
    conversationId: string | null;
    workspace: string | null;
    sequence: number;
    viewOnly: boolean;
  }) => invokeCommand<void>('report_voice_context', args),

  prepareCompanionMemoryPanel: () => invokeCommand<void>('prepare_companion_memory_panel'),

  placeCompanionMemoryPanel: <T extends Record<string, unknown>>(args: T) =>
    invokeCommand<void>('place_companion_memory_panel', args),

  showCompanionMemoryPanel: <T extends Record<string, unknown>>(args: T) =>
    invokeCommand<boolean>('show_companion_memory_panel', args),

  hideCompanionMemoryPanel: <T extends Record<string, unknown>>(args: T) =>
    invokeCommand<boolean>('hide_companion_memory_panel', args),

  async popupContextMenu(entries: ContextMenuEntry[]): Promise<void> {
    if (!isTauriRuntime()) return;
    const [{ Menu }, { getCurrentWindow }] = await Promise.all([
      import('@tauri-apps/api/menu'),
      import('@tauri-apps/api/window'),
    ]);
    const menu = await Menu.new({ items: entries });
    await menu.popup(undefined, getCurrentWindow());
  },

  async webviewLabel(): Promise<string> {
    if (!isTauriRuntime()) return 'browser';
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    return getCurrentWindow().label;
  },

  webui: {
    getStatus: <T>() => (isTauriRuntime() ? tauriWebuiGetStatus<T>() : Promise.resolve(undefined as T)),
    start: <T>() => (isTauriRuntime() ? tauriWebuiStart<T>() : Promise.resolve(undefined as T)),
    stop: <T>() => (isTauriRuntime() ? tauriWebuiStop<T>() : Promise.resolve(undefined as T)),
    onStatusChanged: <T>(listener: (status: T) => void) =>
      tauriEvents.listen<T>('webui://status-changed', listener),
  },
};