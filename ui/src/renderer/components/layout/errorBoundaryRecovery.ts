import { isDomCommitFailure } from './routeErrorRecovery';

/** A DOM commit crash is recoverable by remounting the failed subtree once. */
export const DOM_COMMIT_RECOVERY_DELAY_MS = 0;
/** Re-arm auto recovery only after the remounted subtree stayed healthy. */
export const DOM_COMMIT_RECOVERY_REARM_MS = 5_000;

export type DomCommitRecoveryOptions = {
  /** Remount the failed subtree (the boundary has already unmounted it). */
  onRecover: () => void;
  recoveryDelayMs?: number;
  rearmMs?: number;
};

/**
 * One-shot remount recovery for "an external DOM writer detached nodes React
 * still owns" commit crashes (browser automation UIA writes, translate
 * extensions, …).
 *
 * The boundary has already unmounted the stale subtree when this controller
 * asks for a remount, so rendering the children again starts from a clean DOM.
 * The latch allows exactly one attempt and only re-arms after the remount stays
 * healthy for a while, so a persistent crash cannot become a render loop.
 *
 * Shared by the route boundary and the settings-rail boundary so both surfaces
 * recover from the same failure mode.
 */
export class DomCommitRecovery {
  private attempted = false;
  private recoveryTimer: ReturnType<typeof setTimeout> | null = null;
  private rearmTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly onRecover: () => void;
  private readonly recoveryDelayMs: number;
  private readonly rearmMs: number;

  constructor({
    onRecover,
    recoveryDelayMs = DOM_COMMIT_RECOVERY_DELAY_MS,
    rearmMs = DOM_COMMIT_RECOVERY_REARM_MS,
  }: DomCommitRecoveryOptions) {
    this.onRecover = onRecover;
    this.recoveryDelayMs = recoveryDelayMs;
    this.rearmMs = rearmMs;
  }

  /** Claims one automatic remount for `error`; returns false when not eligible. */
  maybeSchedule(error: unknown): boolean {
    if (this.attempted || !isDomCommitFailure(error)) {
      return false;
    }
    this.attempted = true;
    this.clearTimers();
    this.recoveryTimer = setTimeout(() => {
      this.recoveryTimer = null;
      this.onRecover();
      this.rearmTimer = setTimeout(() => {
        this.rearmTimer = null;
        this.attempted = false;
      }, this.rearmMs);
    }, this.recoveryDelayMs);
    return true;
  }

  /** Clears the latch and pending timers (navigation or manual retry). */
  reset(): void {
    this.attempted = false;
    this.clearTimers();
  }

  /** Stops pending timers when the owning boundary unmounts. */
  dispose(): void {
    this.clearTimers();
  }

  private clearTimers(): void {
    if (this.recoveryTimer !== null) {
      clearTimeout(this.recoveryTimer);
      this.recoveryTimer = null;
    }
    if (this.rearmTimer !== null) {
      clearTimeout(this.rearmTimer);
      this.rearmTimer = null;
    }
  }
}
