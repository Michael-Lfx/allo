/**
 * Dev-oriented timing for the Guid → conversation transition. No telemetry:
 * durations are only written to console.debug so the send → reveal latency
 * budget can be inspected while iterating locally.
 *
 * Usage: `guidTransitionStart()` when the send begins, `guidTransitionMark()`
 * at each milestone (create resolved / configure settled / navigate
 * dispatched), and `guidTransitionEnd(outcome)` when the pending overlay
 * settles (wired to the transition controller's onSettled).
 */

export type GuidTransitionMark =
  | 'createResolved'
  | 'configureSettled'
  | 'navigateDispatched'
  /** Destination page mounted and began consuming the initial message —
   * splits the post-navigation wait into mount (before) vs POST (after). */
  | 'destinationMounted';
export type GuidTransitionOutcome = 'revealed' | 'timeout' | 'aborted';

type TransitionSession = {
  t0: number;
  last: number;
  marks: Array<[GuidTransitionMark, number]>;
};

let session: TransitionSession | null = null;

const now = (): number =>
  typeof performance !== 'undefined' && typeof performance.now === 'function' ? performance.now() : Date.now();

const formatDelta = (ms: number): string => `+${ms.toFixed(0)}ms`;

export const guidTransitionStart = (): void => {
  session = { t0: now(), last: now(), marks: [] };
};

export const guidTransitionMark = (mark: GuidTransitionMark): void => {
  if (!session) return;
  const t = now();
  session.marks.push([mark, t]);
  console.debug('[guid-transition]', mark, formatDelta(t - session.t0), `(Δ ${formatDelta(t - session.last)})`);
  session.last = t;
};

export const guidTransitionEnd = (outcome: GuidTransitionOutcome): void => {
  if (!session) return;
  const total = now() - session.t0;
  console.debug('[guid-transition]', `settled:${outcome}`, `total ${formatDelta(total)}`);
  session = null;
};
