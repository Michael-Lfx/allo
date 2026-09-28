import { NODE_DEFAULT_SIZE } from '@oc/constant/canvas';
import { CanvasNodeType } from '@oc/types/canvas';

export const MINI_SCALE_MIN = 0.2;
export const MINI_SCALE_MAX = 2;
export const MAX_SHOT_IMAGE_REFS = 9;
export const MAX_SHOT_AUDIO_REFS = 3;

export const IMAGE_NODE = {
  w: NODE_DEFAULT_SIZE[CanvasNodeType.Image].width,
  h: NODE_DEFAULT_SIZE[CanvasNodeType.Image].height,
};
export const AUDIO_NODE = {
  w: NODE_DEFAULT_SIZE[CanvasNodeType.Audio].width,
  h: NODE_DEFAULT_SIZE[CanvasNodeType.Audio].height,
};
export const VIDEO_NODE = {
  w: NODE_DEFAULT_SIZE[CanvasNodeType.Video].width,
  h: NODE_DEFAULT_SIZE[CanvasNodeType.Video].height,
};

export type MiniPoint = { x: number; y: number; w?: number; h?: number };
export type MiniViewport = { x: number; y: number; k: number };
export type MiniRect = MiniPoint & { w: number; h: number };

export const VIDEO_NODE_ID = 'video';

export function imageNodeId(slot: number): string {
  return `image-${slot}`;
}

export function audioNodeId(slot: number): string {
  return `audio-${slot}`;
}

export function clampScale(scale: number): number {
  return Math.min(MINI_SCALE_MAX, Math.max(MINI_SCALE_MIN, scale));
}

export function clampViewport(viewport: MiniViewport): MiniViewport {
  return {
    x: Number.isFinite(viewport.x) ? viewport.x : 36,
    y: Number.isFinite(viewport.y) ? viewport.y : 28,
    k: clampScale(Number.isFinite(viewport.k) && viewport.k > 0 ? viewport.k : 1),
  };
}

export function defaultNodePositions(
  imageSlots: number[],
  audioSlots: number[]
): Record<string, MiniPoint> {
  const out: Record<string, MiniPoint> = {};
  imageSlots.forEach((slot, index) => {
    out[imageNodeId(slot)] = {
      x: 48,
      y: 48 + index * (IMAGE_NODE.h + 36),
      w: IMAGE_NODE.w,
      h: IMAGE_NODE.h,
    };
  });
  const audioTop =
    48 + imageSlots.length * (IMAGE_NODE.h + 36) + (imageSlots.length > 0 ? 24 : 0);
  audioSlots.forEach((slot, index) => {
    out[audioNodeId(slot)] = {
      x: 48,
      y: audioTop + index * (AUDIO_NODE.h + 28),
      w: AUDIO_NODE.w,
      h: AUDIO_NODE.h,
    };
  });
  out[VIDEO_NODE_ID] = {
    x: 48 + IMAGE_NODE.w + 120,
    y: 56,
    w: VIDEO_NODE.w,
    h: VIDEO_NODE.h,
  };
  return out;
}

export function mergePositions(
  defaults: Record<string, MiniPoint>,
  saved?: Record<string, MiniPoint> | null
): Record<string, MiniPoint> {
  const next = { ...defaults };
  if (!saved) return next;
  for (const [id, point] of Object.entries(saved)) {
    if (!point) continue;
    if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) continue;
    const current = next[id] ?? { x: point.x, y: point.y };
    next[id] = {
      x: point.x,
      y: point.y,
      w: Number.isFinite(point.w) ? point.w : current.w,
      h: Number.isFinite(point.h) ? point.h : current.h,
    };
  }
  return next;
}

export function nodeSize(kind: 'image' | 'audio' | 'video'): { w: number; h: number } {
  if (kind === 'video') return VIDEO_NODE;
  if (kind === 'audio') return AUDIO_NODE;
  return IMAGE_NODE;
}

export function fitViewport(
  rects: MiniRect[],
  viewW: number,
  viewH: number,
  padding = 72
): MiniViewport {
  if (rects.length === 0 || viewW < 8 || viewH < 8) {
    return { x: 32, y: 28, k: 1 };
  }
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const rect of rects) {
    minX = Math.min(minX, rect.x);
    minY = Math.min(minY, rect.y);
    maxX = Math.max(maxX, rect.x + rect.w);
    maxY = Math.max(maxY, rect.y + rect.h);
  }
  const width = Math.max(1, maxX - minX);
  const height = Math.max(1, maxY - minY);
  const k = clampScale(
    Math.min((viewW - padding * 2) / width, (viewH - padding * 2) / height, 1.15)
  );
  return {
    x: (viewW - width * k) / 2 - minX * k,
    y: (viewH - height * k) / 2 - minY * k,
    k,
  };
}

export function nextSlot(slots: Array<{ slot: number }>): number {
  return slots.reduce((max, item) => Math.max(max, item.slot), 0) + 1;
}
