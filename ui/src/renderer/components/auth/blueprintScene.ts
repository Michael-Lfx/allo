/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  BlueprintRouteStep,
  IntentFieldMode,
  IntentFieldPhase,
} from './authTypes';

export const BLUEPRINT_VIEWBOX = '0 0 1000 680';

export type BlueprintFragmentVariant =
  | 'command-lines'
  | 'file-sheet'
  | 'terminal-rows'
  | 'browser-pane'
  | 'diff-block'
  | 'table-grid'
  | 'summary-bars'
  | 'review-check';

export type BlueprintFragmentRole = 'primary' | 'companion';
export type BlueprintDetailKind = 'line' | 'dot' | 'check';
export type BlueprintDetailEmphasis = 'quiet' | 'accent';

export interface BlueprintDetail {
  kind: BlueprintDetailKind;
  revealAt: BlueprintRouteStep;
  x: number;
  y: number;
  width?: number;
  emphasis?: BlueprintDetailEmphasis;
}

export interface BlueprintFragment {
  id: string;
  role: BlueprintFragmentRole;
  variant: BlueprintFragmentVariant;
  routeStep: BlueprintRouteStep;
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  details?: readonly BlueprintDetail[];
}

export interface BlueprintRouteSegment {
  id: string;
  step: Exclude<BlueprintRouteStep, 0>;
  path: string;
  fragmentIds: readonly string[];
}

export interface BlueprintSupportLine {
  id: string;
  path: string;
  fragmentIds: readonly string[];
}

export type BlueprintAmbientMarkKind = 'locator' | 'completion';

export interface BlueprintAmbientMark {
  id: string;
  kind: BlueprintAmbientMarkKind;
  routeStep: BlueprintRouteStep;
  x: number;
  y: number;
  size?: number;
}

/**
 * Hand-placed documents keep the composition editorial. Their route step is
 * the single source of truth for path, opacity, and backplate arrival.
 */
export const BLUEPRINT_FRAGMENTS: readonly BlueprintFragment[] = [
  {
    id: 'browser-result', role: 'primary', variant: 'browser-pane', routeStep: 1,
    x: 34, y: 394, width: 146, height: 64, rotation: -3.5,
    details: [
      { kind: 'line', revealAt: 1, x: 12, y: 49, width: 58, emphasis: 'quiet' },
      { kind: 'dot', revealAt: 1, x: 108, y: 14, emphasis: 'accent' },
    ],
  },
  {
    id: 'command', role: 'primary', variant: 'command-lines', routeStep: 2,
    x: 258, y: 252, width: 148, height: 58, rotation: -1.6,
    details: [
      { kind: 'dot', revealAt: 2, x: 52, y: 14, emphasis: 'accent' },
      { kind: 'line', revealAt: 3, x: 12, y: 45, width: 42, emphasis: 'quiet' },
    ],
  },
  {
    id: 'file', role: 'primary', variant: 'file-sheet', routeStep: 2,
    x: 122, y: 286, width: 94, height: 70, rotation: 2.8,
    details: [
      { kind: 'line', revealAt: 2, x: 10, y: 49, width: 32, emphasis: 'quiet' },
      { kind: 'check', revealAt: 3, x: 72, y: 52, emphasis: 'accent' },
      { kind: 'dot', revealAt: 4, x: 68, y: 46, emphasis: 'accent' },
    ],
  },
  {
    id: 'terminal', role: 'primary', variant: 'terminal-rows', routeStep: 3,
    x: 408, y: 126, width: 136, height: 84, rotation: -2.4,
    details: [
      { kind: 'line', revealAt: 3, x: 16, y: 73, width: 58, emphasis: 'quiet' },
    ],
  },
  {
    id: 'browser', role: 'primary', variant: 'browser-pane', routeStep: 4,
    x: 504, y: 410, width: 146, height: 68, rotation: 2.6,
    details: [
      { kind: 'dot', revealAt: 5, x: 130, y: 14, emphasis: 'accent' },
      { kind: 'check', revealAt: 5, x: 116, y: 45, emphasis: 'accent' },
    ],
  },
  {
    id: 'review', role: 'primary', variant: 'review-check', routeStep: 5,
    x: 582, y: 282, width: 116, height: 62, rotation: -2.1,
    details: [
      { kind: 'line', revealAt: 5, x: 12, y: 50, width: 38, emphasis: 'quiet' },
      { kind: 'dot', revealAt: 6, x: 94, y: 18, emphasis: 'accent' },
    ],
  },
  {
    id: 'file-edge', role: 'primary', variant: 'file-sheet', routeStep: 5,
    x: 684, y: 92, width: 94, height: 62, rotation: -0.8,
    details: [
      { kind: 'line', revealAt: 5, x: 12, y: 46, width: 40, emphasis: 'quiet' },
    ],
  },
  {
    id: 'diff', role: 'primary', variant: 'diff-block', routeStep: 6,
    x: 708, y: 356, width: 116, height: 62, rotation: 2.1,
    details: [
      { kind: 'line', revealAt: 6, x: 14, y: 56, width: 58, emphasis: 'quiet' },
    ],
  },
  {
    id: 'table', role: 'primary', variant: 'table-grid', routeStep: 6,
    x: 796, y: 148, width: 100, height: 58, rotation: 0.4,
    details: [
      { kind: 'dot', revealAt: 7, x: 12, y: 12, emphasis: 'quiet' },
      { kind: 'check', revealAt: 7, x: 72, y: 26, emphasis: 'accent' },
    ],
  },
  {
    id: 'review-summary', role: 'primary', variant: 'summary-bars', routeStep: 7,
    x: 834, y: 226, width: 122, height: 74, rotation: 1.9,
    details: [
      { kind: 'check', revealAt: 7, x: 96, y: 55, emphasis: 'accent' },
    ],
  },
  {
    id: 'companion-terminal', role: 'companion', variant: 'terminal-rows', routeStep: 0,
    x: 226, y: 172, width: 96, height: 52, rotation: 1.5,
  },
  {
    id: 'companion-diff', role: 'companion', variant: 'diff-block', routeStep: 0,
    x: 424, y: 278, width: 108, height: 54, rotation: -1.8,
  },
  {
    id: 'companion-file', role: 'companion', variant: 'file-sheet', routeStep: 0,
    x: 276, y: 420, width: 96, height: 54, rotation: -2.6,
    details: [
      { kind: 'line', revealAt: 0, x: 10, y: 39, width: 36, emphasis: 'quiet' },
      { kind: 'dot', revealAt: 0, x: 74, y: 13, emphasis: 'quiet' },
    ],
  },
  {
    id: 'companion-table', role: 'companion', variant: 'table-grid', routeStep: 0,
    x: 720, y: 208, width: 92, height: 52, rotation: 0.5,
  },
  {
    id: 'companion-summary', role: 'companion', variant: 'summary-bars', routeStep: 0,
    x: 488, y: 500, width: 104, height: 54, rotation: -1.4,
  },
] as const;

/** Each event owns one short segment instead of drawing one global route. */
export const BLUEPRINT_ROUTE_SEGMENTS: readonly BlueprintRouteSegment[] = [
  { id: 'intent', step: 1, path: 'M 70 474 H 140 L 184 438', fragmentIds: ['browser-result'] },
  { id: 'prepare', step: 2, path: 'M 184 438 H 242 L 292 344 H 360', fragmentIds: ['command', 'file'] },
  { id: 'inspect', step: 3, path: 'M 360 344 L 408 302 V 232 H 484', fragmentIds: ['terminal'] },
  { id: 'compose', step: 4, path: 'M 484 232 L 528 280 V 394 H 618', fragmentIds: ['browser'] },
  { id: 'review', step: 5, path: 'M 618 394 L 660 352 V 220 H 724', fragmentIds: ['review', 'file-edge'] },
  { id: 'summarize', step: 6, path: 'M 724 220 L 772 244 V 338 H 892', fragmentIds: ['diff', 'table'] },
  { id: 'verify', step: 7, path: 'M 892 338 L 930 302 V 266', fragmentIds: ['review-summary'] },
] as const;

/** Static editorial connectors, separate from the primary execution route. */
export const BLUEPRINT_SUPPORT_LINES: readonly BlueprintSupportLine[] = [
  {
    id: 'support-upper-left', path: 'M 322 200 L 348 200 L 356 166 L 408 173',
    fragmentIds: ['companion-terminal', 'terminal'],
  },
  {
    id: 'support-upper-right', path: 'M 778 133 L 796 163',
    fragmentIds: ['file-edge', 'table'],
  },
  {
    id: 'support-lower', path: 'M 373 445 H 438 V 501 H 487',
    fragmentIds: ['companion-file', 'companion-summary'],
  },
] as const;

export const BLUEPRINT_MARKS = [
  { x: 70, y: 474, size: 14 },
  { x: 184, y: 438, size: 9 },
  { x: 360, y: 344, size: 11 },
  { x: 484, y: 232, size: 8 },
  { x: 618, y: 394, size: 14 },
  { x: 724, y: 220, size: 10 },
  { x: 892, y: 338, size: 8 },
  { x: 930, y: 266, size: 14 },
] as const;

export const BLUEPRINT_AMBIENT_MARKS: readonly BlueprintAmbientMark[] = [
  { id: 'locator-mid-left', kind: 'locator', routeStep: 1, x: 236, y: 238, size: 10 },
  { id: 'locator-mid-right', kind: 'locator', routeStep: 3, x: 560, y: 372, size: 12 },
  { id: 'locator-lower', kind: 'locator', routeStep: 5, x: 454, y: 392, size: 8 },
  { id: 'completion-anchor', kind: 'completion', routeStep: 7, x: 782, y: 274, size: 12 },
] as const;

export const BLUEPRINT_VERIFY_PATH = BLUEPRINT_ROUTE_SEGMENTS[6].path;
export const BLUEPRINT_CURSOR_START = { x: 892, y: 338 } as const;
export const BLUEPRINT_CURSOR_END = { x: 930, y: 266 } as const;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** OTP entry advances the route at four deliberate checkpoints, not per keypress. */
export const OTP_BLUEPRINT_CHECKPOINTS = [1, 3, 5, 6] as const;

export const getOtpBlueprintCheckpoint = (codeLength: number): 0 | 1 | 2 | 3 | 4 => {
  const length = clamp(Math.floor(codeLength), 0, 6);
  return OTP_BLUEPRINT_CHECKPOINTS.reduce<number>(
    (checkpoint, threshold, index) => (length >= threshold ? index + 1 : checkpoint),
    0
  ) as 0 | 1 | 2 | 3 | 4;
};

export const getBlueprintRouteStep = (
  mode: IntentFieldMode,
  phase: IntentFieldPhase,
  inputEnergy: number,
  explicitStep?: BlueprintRouteStep,
): BlueprintRouteStep => {
  if (explicitStep !== undefined) return explicitStep;
  if (phase === 'success' || phase === 'verifying') return 7;

  const energy = clamp(inputEnergy, 0, 1);
  if (mode === 'cloud') {
    if (phase === 'code-sent') return 2;
    return energy > 0 ? 1 : 0;
  }

  if (phase === 'idle') return 0;
  return Math.min(3, Math.max(1, Math.ceil(energy * 3))) as BlueprintRouteStep;
};

export const getBlueprintArrivedFragmentIds = (step: BlueprintRouteStep) => {
  return BLUEPRINT_FRAGMENTS
    .filter((fragment) => fragment.role === 'primary' && fragment.routeStep > 0 && fragment.routeStep <= step)
    .map((fragment) => fragment.id);
};

export const getBlueprintFocusIds = (x: number, y: number, limit = 2) => {
  const candidates = BLUEPRINT_FRAGMENTS
    .filter((fragment) => fragment.role === 'primary')
    .map((fragment) => ({
      id: fragment.id,
      distance: Math.hypot(fragment.x + fragment.width / 2 - x, fragment.y + fragment.height / 2 - y),
    }))
    .sort((left, right) => left.distance - right.distance);

  return candidates
    .filter((candidate) => candidate.distance < 250)
    .slice(0, limit)
    .map((candidate) => candidate.id);
};
