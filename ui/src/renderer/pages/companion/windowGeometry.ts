

export interface GeomRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface GeomSize {
  width: number;
  height: number;
}

export const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), Math.max(lo, hi));

/** Intersection area of two rects (0 when disjoint). */
export const overlapArea = (a: GeomRect, b: GeomRect): number =>
  Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)) *
  Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y));

/**
 * Pick the item whose bounds overlap `anchor` the most. Ties go to the earlier
 * item; when nothing overlaps the first item is returned; empty input → null.
 */
export function pickHost<T>(anchor: GeomRect, items: T[], boundsOf: (item: T) => GeomRect): T | null {
  if (items.length === 0) return null;
  return items.reduce((best, item) => (overlapArea(anchor, boundsOf(item)) > overlapArea(anchor, boundsOf(best)) ? item : best));
}

/**
 * Placement for an in-place companion-window resize: the bottom edge stays put and
 * the window grows/shrinks around its horizontal center, then the result is
 * clamped into the monitor the old rect overlaps most — a taller window must
 * never sink below the screen (if the window exceeds the monitor itself, the
 * top edge pins to the monitor's top). Used only at actual size-change moments, so it
 * never disturbs a user's deliberate half-off-screen placement during normal
 * position restores. All values in physical px.
 */
export function placeResizedWindow(oldRect: GeomRect, newSize: GeomSize, monitors: GeomRect[]): { x: number; y: number } {
  let x = oldRect.x + Math.round((oldRect.width - newSize.width) / 2);
  let y = oldRect.y + (oldRect.height - newSize.height);
  const monitor = pickHost(oldRect, monitors, (m) => m);
  if (monitor) {
    x = clamp(x, monitor.x, monitor.x + monitor.width - newSize.width);
    y = clamp(y, monitor.y, monitor.y + monitor.height - newSize.height);
  }
  return { x, y };
}

/** Axis-aligned overlap in px. Zero when disjoint. */
export const overlapExtent = (a: GeomRect, b: GeomRect): { x: number; y: number } => ({
  x: Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)),
  y: Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y)),
});

/**
 * True when `rect` still has a grabable slice on some display. Multi-monitor
 * layouts may have gaps; a window parked in a gap or past the virtual desktop
 * is not reachable.
 */
export function isReachableOnDisplays(rect: GeomRect, displays: GeomRect[], minEdgePx = 1): boolean {
  if (displays.length === 0) return true;
  const min = Math.max(1, minEdgePx);
  return displays.some((display) => {
    const o = overlapExtent(rect, display);
    return o.x >= min && o.y >= min;
  });
}

/** Display whose clamped-point distance to `rect`'s center is smallest. */
export function nearestDisplay(rect: GeomRect, displays: GeomRect[]): GeomRect | null {
  if (displays.length === 0) return null;
  const cx = rect.x + rect.width / 2;
  const cy = rect.y + rect.height / 2;
  let best = displays[0];
  let bestDist = Number.POSITIVE_INFINITY;
  for (const display of displays) {
    const px = clamp(cx, display.x, display.x + display.width);
    const py = clamp(cy, display.y, display.y + display.height);
    const dist = (cx - px) * (cx - px) + (cy - py) * (cy - py);
    if (dist < bestDist) {
      bestDist = dist;
      best = display;
    }
  }
  return best;
}

/**
 * Keep a companion window fully inside one display: the display it overlaps
 * most, otherwise the nearest. The pet is small; straddling bezels or floating
 * in a monitor gap is never a valid rest pose. When the window is larger than
 * the host (expanded chat, short display), the top-left pins to the host.
 */
export function clampRectToDisplays(rect: GeomRect, displays: GeomRect[]): { x: number; y: number } {
  if (displays.length === 0) return { x: rect.x, y: rect.y };
  const overlapping = pickHost(rect, displays, (d) => d);
  const host =
    overlapping && overlapArea(rect, overlapping) > 0 ? overlapping : nearestDisplay(rect, displays);
  if (!host) return { x: rect.x, y: rect.y };
  return {
    x: clamp(rect.x, host.x, host.x + host.width - rect.width),
    y: clamp(rect.y, host.y, host.y + host.height - rect.height),
  };
}
