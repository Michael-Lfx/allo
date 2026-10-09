import { clampRectToDisplays, isReachableOnDisplays, type GeomRect } from './windowGeometry';

/** Minimal Tauri monitor shape used for work-area clamping. */
export interface TauriMonitorLike {
  position: { x: number; y: number };
  size: { width: number; height: number };
  workArea?: {
    position: { x: number; y: number };
    size: { width: number; height: number };
  };
}

/** Prefer the OS work area (taskbar/dock excluded); fall back to the full display. */
export function workAreasFromMonitors(monitors: TauriMonitorLike[]): GeomRect[] {
  return monitors.map((monitor) => {
    const work = monitor.workArea;
    if (work && work.size.width > 0 && work.size.height > 0) {
      return {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width,
        height: work.size.height,
      };
    }
    return {
      x: monitor.position.x,
      y: monitor.position.y,
      width: monitor.size.width,
      height: monitor.size.height,
    };
  });
}

export async function loadCompanionWorkAreas(): Promise<GeomRect[]> {
  const { availableMonitors } = await import('@tauri-apps/api/window');
  return workAreasFromMonitors(await availableMonitors());
}

/** Fully inside one connected work area — the rest pose for restore / persist. */
export function clampSavedCompanionPosition(
  saved: { x: number; y: number },
  size: { width: number; height: number },
  monitors: TauriMonitorLike[]
): { x: number; y: number } {
  return clampRectToDisplays(
    { x: saved.x, y: saved.y, width: size.width, height: size.height },
    workAreasFromMonitors(monitors)
  );
}

/**
 * Snap a companion window onto a connected work area.
 * `force` = always fully inside one display (drag end / restore).
 * Otherwise only correct a window that has left every display.
 */
export async function snapCompanionWindow(opts?: { force?: boolean }): Promise<{ x: number; y: number } | null> {
  const { getCurrentWindow, PhysicalPosition } = await import('@tauri-apps/api/window');
  const win = getCurrentWindow();
  const [pos, size, displays] = await Promise.all([win.outerPosition(), win.outerSize(), loadCompanionWorkAreas()]);
  if (displays.length === 0) return { x: pos.x, y: pos.y };
  const rect: GeomRect = { x: pos.x, y: pos.y, width: size.width, height: size.height };
  if (!opts?.force && isReachableOnDisplays(rect, displays)) return { x: pos.x, y: pos.y };
  const next = clampRectToDisplays(rect, displays);
  if (next.x !== pos.x || next.y !== pos.y) {
    await win.setPosition(new PhysicalPosition(next.x, next.y));
  }
  return next;
}
