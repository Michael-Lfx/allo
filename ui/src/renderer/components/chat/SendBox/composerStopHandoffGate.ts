const EDIT_SUBMIT_STOP_HANDOFF_MS = 500;

export interface ComposerStopHandoffGate {
  armAfterEditSubmit(): void;
  shouldIgnoreStop(clickDetail: number): boolean;
}

/** Prevent a double-click's second click from crossing from Send into Stop. */
export const createComposerStopHandoffGate = (
  now: () => number = Date.now
): ComposerStopHandoffGate => {
  let blockedUntil = 0;
  return {
    armAfterEditSubmit: () => {
      blockedUntil = now() + EDIT_SUBMIT_STOP_HANDOFF_MS;
    },
    shouldIgnoreStop: (clickDetail) => clickDetail >= 2 && now() < blockedUntil,
  };
};
