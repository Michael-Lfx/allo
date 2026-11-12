/**
 * Unified window, monitor, and window-lifecycle adapter.
 *
 * Renderer code talks to this module instead of `@tauri-apps/api/window` and
 * `@tauri-apps/api/webviewWindow`. Native Tauri objects never leave this file:
 * every operation is expressed in the app-owned `WindowHandle` / `MonitorInfo`
 * types. In the WebUI host each method degrades to a safe no-op or empty value.
 * Desktop handle operations reject on failure so callers can retry or recover.
 */
import type { Window as NativeWindow } from '@tauri-apps/api/window';
import type { Unsubscribe } from './tauriEvents';
import { isTauriRuntime } from './tauriRuntime';

export type { Unsubscribe };

export interface PhysicalPoint {
  x: number;
  y: number;
}

export interface PhysicalSize {
  width: number;
  height: number;
}

export interface PhysicalRect extends PhysicalPoint, PhysicalSize {}

export interface MonitorInfo {
  name?: string;
  position: PhysicalPoint;
  size: PhysicalSize;
  workArea: {
    position: PhysicalPoint;
    size: PhysicalSize;
  };
  scaleFactor: number;
}

export interface WindowCreateSpec {
  label: string;
  url?: string;
  title?: string;
  width?: number;
  height?: number;
}

export interface WindowHandle {
  readonly label: string;
  close(): Promise<void>;
  minimize(): Promise<void>;
  maximize(): Promise<void>;
  unmaximize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  isMaximized(): Promise<boolean>;
  isFocused(): Promise<boolean>;
  isVisible(): Promise<boolean>;
  hide(): Promise<void>;
  show(): Promise<void>;
  unminimize(): Promise<void>;
  setFocus(): Promise<void>;
  setTitle(title: string): Promise<void>;
  setPosition(position: PhysicalPoint): Promise<void>;
  setSize(size: PhysicalSize): Promise<void>;
  scaleFactor(): Promise<number>;
  startDragging(): Promise<void>;
  outerPosition(): Promise<PhysicalPoint>;
  outerSize(): Promise<PhysicalSize>;
  setIgnoreCursorEvents(ignore: boolean): Promise<void>;
  onFocusChanged(listener: (focused: boolean) => void): Promise<Unsubscribe>;
  onMoved(listener: () => void): Promise<Unsubscribe>;
  onResized(listener: () => void): Promise<Unsubscribe>;
  onScaleChanged(listener: () => void): Promise<Unsubscribe>;
}

const noop = (): void => {};

const EMPTY_POINT: PhysicalPoint = { x: 0, y: 0 };
const EMPTY_SIZE: PhysicalSize = { width: 0, height: 0 };
const UNIT_SCALE = 1;

const noOpWindow = (label: string): WindowHandle => ({
  label,
  close: async () => {},
  minimize: async () => {},
  maximize: async () => {},
  unmaximize: async () => {},
  toggleMaximize: async () => {},
  isMaximized: async () => false,
  isFocused: async () => false,
  isVisible: async () => false,
  hide: async () => {},
  show: async () => {},
  unminimize: async () => {},
  setFocus: async () => {},
  setTitle: async () => {},
  setPosition: async () => {},
  setSize: async () => {},
  scaleFactor: async () => UNIT_SCALE,
  startDragging: async () => {},
  outerPosition: async () => ({ ...EMPTY_POINT }),
  outerSize: async () => ({ ...EMPTY_SIZE }),
  setIgnoreCursorEvents: async () => {},
  onFocusChanged: async () => noop,
  onMoved: async () => noop,
  onResized: async () => noop,
  onScaleChanged: async () => noop,
});

/** Window label needed before the async Tauri API finishes loading. */
const currentWindowLabel = (): string => {
  const metadata = (
    window as Window & {
      __TAURI_INTERNALS__?: {
        metadata?: { currentWindow?: { label?: string } };
      };
    }
  ).__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
  return metadata ?? 'main';
};

const wrapWindow = (native: NativeWindow): WindowHandle => {
  const call = async <T>(operation: (window: NativeWindow) => Promise<T>): Promise<T> =>
    operation(native);

  return {
    get label() {
      return native.label;
    },
    close: () => call((window) => window.close()),
    minimize: () => call((window) => window.minimize()),
    maximize: () => call((window) => window.maximize()),
    unmaximize: () => call((window) => window.unmaximize()),
    toggleMaximize: () => call((window) => window.toggleMaximize()),
    isMaximized: () => call((window) => window.isMaximized()),
    isFocused: () => call((window) => window.isFocused()),
    isVisible: () => call((window) => window.isVisible()),
    hide: () => call((window) => window.hide()),
    show: () => call((window) => window.show()),
    unminimize: () => call((window) => window.unminimize()),
    setFocus: () => call((window) => window.setFocus()),
    setTitle: (title) => call((window) => window.setTitle(title)),
    setPosition: ({ x, y }) =>
      call(async (window) => {
        const { PhysicalPosition } = await import('@tauri-apps/api/window');
        await window.setPosition(new PhysicalPosition(x, y));
      }),
    setSize: ({ width, height }) =>
      call(async (window) => {
        const { PhysicalSize } = await import('@tauri-apps/api/window');
        await window.setSize(new PhysicalSize(width, height));
      }),
    scaleFactor: () => call((window) => window.scaleFactor()),
    startDragging: () => call((window) => window.startDragging()),
    outerPosition: () =>
      call(async (window) => {
        const position = await window.outerPosition();
        return { x: position.x, y: position.y };
      }),
    outerSize: () =>
      call(async (window) => {
        const size = await window.outerSize();
        return { width: size.width, height: size.height };
      }),
    setIgnoreCursorEvents: (ignore) => call((window) => window.setIgnoreCursorEvents(ignore)),
    onFocusChanged: (listener) =>
      call((window) => window.onFocusChanged(({ payload }) => listener(payload))),
    onMoved: (listener) => call((window) => window.onMoved(listener)),
    onResized: (listener) => call((window) => window.onResized(listener)),
    onScaleChanged: (listener) => call((window) => window.onScaleChanged(listener)),
  };
};

const PROXY_LABEL = 'current';

/**
 * Synchronous handle whose native window is resolved on first use. This keeps
 * `tauriWindows.current()` and `tauriWindows.get(label)` usable during render
 * while still loading the Tauri API lazily.
 */
const lazyWindow = (label: string | undefined, isCurrent: boolean): WindowHandle => {
  let nativeWindow: Promise<NativeWindow> | null = null;
  const resolve = (): Promise<NativeWindow> => {
    if (!nativeWindow) {
      nativeWindow = import('@tauri-apps/api/window')
        .then(async ({ getCurrentWindow, Window }) => {
          if (isCurrent || !label) return getCurrentWindow();
          if (getCurrentWindow().label === label) return getCurrentWindow();
          const window = await Window.getByLabel(label);
          if (!window) throw new Error(`window not found: ${label}`);
          return window;
        })
        .catch((error) => {
          // A later operation may succeed after bridge startup or window creation.
          nativeWindow = null;
          throw error;
        });
    }
    return nativeWindow;
  };
  const call = async <T>(operation: (window: NativeWindow) => Promise<T>): Promise<T> =>
    operation(await resolve());

  return {
    label: label ?? (isTauriRuntime() ? currentWindowLabel() : PROXY_LABEL),
    close: () => call((window) => window.close()),
    minimize: () => call((window) => window.minimize()),
    maximize: () => call((window) => window.maximize()),
    unmaximize: () => call((window) => window.unmaximize()),
    toggleMaximize: () => call((window) => window.toggleMaximize()),
    isMaximized: () => call((window) => window.isMaximized()),
    isFocused: () => call((window) => window.isFocused()),
    isVisible: () => call((window) => window.isVisible()),
    hide: () => call((window) => window.hide()),
    show: () => call((window) => window.show()),
    unminimize: () => call((window) => window.unminimize()),
    setFocus: () => call((window) => window.setFocus()),
    setTitle: (title) => call((window) => window.setTitle(title)),
    setPosition: ({ x, y }) =>
      call(async (window) => {
        const { PhysicalPosition } = await import('@tauri-apps/api/window');
        await window.setPosition(new PhysicalPosition(x, y));
      }),
    setSize: ({ width, height }) =>
      call(async (window) => {
        const { PhysicalSize } = await import('@tauri-apps/api/window');
        await window.setSize(new PhysicalSize(width, height));
      }),
    scaleFactor: () => call((window) => window.scaleFactor()),
    startDragging: () => call((window) => window.startDragging()),
    outerPosition: () =>
      call(async (window) => {
        const position = await window.outerPosition();
        return { x: position.x, y: position.y };
      }),
    outerSize: () =>
      call(async (window) => {
        const size = await window.outerSize();
        return { width: size.width, height: size.height };
      }),
    setIgnoreCursorEvents: (ignore) => call((window) => window.setIgnoreCursorEvents(ignore)),
    onFocusChanged: (listener) =>
      call((window) => window.onFocusChanged(({ payload }) => listener(payload))),
    onMoved: (listener) => call((window) => window.onMoved(listener)),
    onResized: (listener) => call((window) => window.onResized(listener)),
    onScaleChanged: (listener) => call((window) => window.onScaleChanged(listener)),
  };
};

export const tauriWindows = {
  current(): WindowHandle {
    return isTauriRuntime() ? lazyWindow(undefined, true) : noOpWindow(PROXY_LABEL);
  },

  get(label: string): WindowHandle {
    if (!label.trim()) throw new Error('window label must not be empty');
    return isTauriRuntime() ? lazyWindow(label, false) : noOpWindow(label);
  },

  async list(): Promise<WindowHandle[]> {
    if (!isTauriRuntime()) return [];
    try {
      const { getAllWindows } = await import('@tauri-apps/api/window');
      return (await getAllWindows()).map(wrapWindow);
    } catch {
      return [];
    }
  },

  async create(spec: WindowCreateSpec): Promise<WindowHandle> {
    if (!isTauriRuntime()) return noOpWindow(spec.label);
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
    const window = new WebviewWindow(spec.label, {
      url: spec.url,
      title: spec.title,
      width: spec.width,
      height: spec.height,
    });
    return wrapWindow(window);
  },

  async monitors(): Promise<MonitorInfo[]> {
    if (!isTauriRuntime()) return [];
    try {
      const { availableMonitors } = await import('@tauri-apps/api/window');
      return (await availableMonitors()).map((monitor) => ({
        name: monitor.name ?? undefined,
        position: { x: monitor.position.x, y: monitor.position.y },
        size: { width: monitor.size.width, height: monitor.size.height },
        workArea: {
          position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
          size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
        },
        scaleFactor: monitor.scaleFactor,
      }));
    } catch {
      return [];
    }
  },

  async onMaximizedChanged(listener: (isMaximized: boolean) => void): Promise<Unsubscribe> {
    if (!isTauriRuntime()) return noop;
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const window = getCurrentWindow();
      return await window.onResized(() => {
        void window.isMaximized().then(listener).catch(noop);
      });
    } catch {
      return noop;
    }
  },

  async isAppFocused(): Promise<boolean> {
    if (!isTauriRuntime()) {
      return typeof document !== 'undefined' && document.hasFocus();
    }
    try {
      const { getAllWindows } = await import('@tauri-apps/api/window');
      const windows = await getAllWindows();
      const focused = await Promise.all(
        windows.map((window) => window.isFocused().catch(() => false))
      );
      return focused.some(Boolean);
    } catch {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        return await getCurrentWindow().isFocused();
      } catch {
        return typeof document !== 'undefined' && document.hasFocus();
      }
    }
  },
};
