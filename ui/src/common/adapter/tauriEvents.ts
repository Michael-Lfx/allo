/**
 * Unified Tauri event adapter.
 *
 * Renderer code must not import `@tauri-apps/api/event` or call
 * `getCurrentWindow().listen` directly. This module owns the async subscription
 * race, WebUI no-op behavior, and window-label validation. Desktop event sends
 * reject on failure; callers decide whether delivery is required or best-effort.
 */
import { isTauriRuntime } from './tauriRuntime';

export type Unsubscribe = () => void;
export type TauriEventHandler<T> = (payload: T) => void;

const noop = (): void => {};

const subscribe = async <T>(
  name: string,
  handler: TauriEventHandler<T>,
  subscribeNative: () => Promise<Unsubscribe>
): Promise<Unsubscribe> => {
  if (!name.trim()) return noop;
  if (!isTauriRuntime()) return noop;
  try {
    return await subscribeNative();
  } catch {
    return noop;
  }
};

export const tauriEvents = {
  listen<T>(name: string, handler: TauriEventHandler<T>): Promise<Unsubscribe> {
    return subscribe(name, handler, async () => {
      const { listen } = await import('@tauri-apps/api/event');
      return listen<T>(name, (event) => handler(event.payload));
    });
  },

  listenCurrentWindow<T>(name: string, handler: TauriEventHandler<T>): Promise<Unsubscribe> {
    return subscribe(name, handler, async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      return getCurrentWindow().listen<T>(name, ({ payload }) => handler(payload));
    });
  },

  async emit<T>(name: string, payload: T): Promise<void> {
    if (!name.trim() || !isTauriRuntime()) return;
    const { emit } = await import('@tauri-apps/api/event');
    await emit(name, payload);
  },

  async emitTo<T>(windowLabel: string, name: string, payload: T): Promise<void> {
    if (!windowLabel.trim()) throw new Error('window label must not be empty');
    if (!name.trim() || !isTauriRuntime()) return;
    const { emitTo } = await import('@tauri-apps/api/event');
    await emitTo(windowLabel, name, payload);
  },
};

/**
 * Subscribe without leaking the async Tauri listener race to React effects.
 * The returned function is safe to call before the Tauri listener resolves.
 */
export function listenTauriEvent<T>(name: string, handler: TauriEventHandler<T>): Unsubscribe {
  let disposed = false;
  let unsubscribe: Unsubscribe | undefined;
  void tauriEvents
    .listen(name, handler)
    .then((stop) => {
      if (disposed) stop();
      else unsubscribe = stop;
    })
    .catch(noop);
  return () => {
    disposed = true;
    unsubscribe?.();
  };
}

/** Current-window variant with the same late-unsubscribe guarantee. */
export function listenCurrentWindowTauriEvent<T>(
  name: string,
  handler: TauriEventHandler<T>
): Unsubscribe {
  let disposed = false;
  let unsubscribe: Unsubscribe | undefined;
  void tauriEvents
    .listenCurrentWindow(name, handler)
    .then((stop) => {
      if (disposed) stop();
      else unsubscribe = stop;
    })
    .catch(noop);
  return () => {
    disposed = true;
    unsubscribe?.();
  };
}
