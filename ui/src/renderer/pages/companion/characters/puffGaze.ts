import type { Look } from '../bot/engine';
import { clamp } from '../bot/math';

/** Head-turn extents chosen for a desk pet: readable, never behind the limb. */
const YAW_MAX = 16;
const PITCH_MAX = 13;
/** Absolute rest pitch — slightly above the equator so idle looks attentive. */
const PITCH = 10;

/**
 * Pointer-follow target for the desk pet. Unlike bloub's page-entrance tour,
 * the companion never spins a full turn on aim: the figure is already on the
 * desktop, so the look just eases toward the cursor.
 */
export function deskLookTarget(nx: number, ny: number, pointer: boolean): Look {
  return {
    yaw: nx * YAW_MAX,
    pitch: PITCH - ny * PITCH_MAX,
    mix: pointer ? 1 : 0,
    spin: 0,
    wander: pointer ? 0 : 1,
  };
}

export function pointerOffset(box: DOMRect, clientX: number, clientY: number): { nx: number; ny: number } {
  const halfW = Math.max(1, (typeof window !== 'undefined' ? window.innerWidth : box.width) / 2);
  const halfH = Math.max(1, (typeof window !== 'undefined' ? window.innerHeight : box.height) / 2);
  return {
    nx: clamp((clientX - (box.left + box.width / 2)) / halfW, -1, 1),
    ny: clamp((clientY - (box.top + box.height / 2)) / halfH, -1, 1),
  };
}
