export interface RefreshRetryController {
  trigger: () => void;
  dispose: () => void;
}

/** Coalesced retry-until-success loop used by reconciliation refreshes. */
export const createRefreshRetryController = ({
  load,
  delaysMs = [0, 100, 250, 500, 1_000, 2_000, 5_000],
}: {
  load: () => Promise<void>;
  delaysMs?: readonly number[];
}): RefreshRetryController => {
  let disposed = false;
  let running = false;
  let pending = false;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const run = (): void => {
    if (disposed || running) return;
    running = true;
    pending = false;
    void load()
      .then(() => {
        attempt = 0;
      })
      .catch((error) => {
        if (disposed) return;
        console.warn('[useMessageLstCache] Failed to refresh messages after edit-resubmit:', error);
        const delay = delaysMs[Math.min(attempt, delaysMs.length - 1)] ?? 0;
        attempt += 1;
        timer = setTimeout(() => {
          timer = undefined;
          run();
        }, delay);
      })
      .finally(() => {
        running = false;
        if (disposed) return;
        if (pending) {
          if (timer !== undefined) {
            clearTimeout(timer);
            timer = undefined;
          }
          run();
        }
      });
  };

  return {
    trigger: () => {
      if (disposed) return;
      pending = true;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      run();
    },
    dispose: () => {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
    },
  };
};
