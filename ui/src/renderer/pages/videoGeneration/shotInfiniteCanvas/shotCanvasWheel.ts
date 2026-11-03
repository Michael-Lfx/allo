export type ShotCanvasPointerHit = 'canvas' | 'portal' | 'outside';

const PORTAL_SELECTOR = [
  '.oc-portal-host',
  '.ant-modal',
  '.ant-modal-root',
  '.ant-popover',
  '.ant-dropdown',
  '.ant-select-dropdown',
  '.ant-picker-dropdown',
  '.ant-tooltip',
].join(',');

/** Clicking the canvas arms wheel-zoom. Portaled menus keep the current arm. Outside clicks release it. */
export function nextShotCanvasWheelArmed(hit: ShotCanvasPointerHit, armed: boolean): boolean {
  if (hit === 'canvas') return true;
  if (hit === 'portal') return armed;
  return false;
}

export function shotCanvasPointerHit(root: HTMLElement | null, target: EventTarget | null): ShotCanvasPointerHit {
  if (!root || !(target instanceof Node)) return 'outside';
  if (root.contains(target)) return 'canvas';
  const element = target instanceof Element ? target : target.parentElement;
  if (element?.closest(PORTAL_SELECTOR)) return 'portal';
  return 'outside';
}
