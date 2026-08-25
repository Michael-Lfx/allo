/**
 * Serialized, retrying persistence for a server-backed canvas project.
 *
 * The store can emit several mutations during a drag. This controller retains
 * one dirty bit, so it never overlaps PUTs and always follows a failed write
 * with a bounded-backoff retry while the project remains mounted.
 */

export const CANVAS_AUTOSAVE_DEBOUNCE_MS = 800;
export const CANVAS_AUTOSAVE_RETRY_BASE_MS = 1_000;
export const CANVAS_AUTOSAVE_RETRY_MAX_MS = 30_000;

export function canvasAutosaveRetryDelayMs(
  consecutiveFailures: number,
  retryBaseMs = CANVAS_AUTOSAVE_RETRY_BASE_MS,
  retryMaxMs = CANVAS_AUTOSAVE_RETRY_MAX_MS
): number {
  const exponent = Math.min(Math.max(0, consecutiveFailures - 1), 16);
  return Math.min(retryMaxMs, retryBaseMs * 2 ** exponent);
}

export interface CanvasProjectAutosaveController {
  /** Mark the latest canvas state dirty and debounce its remote write. */
  markDirty: () => void;
  /** Cancel a debounce and write the latest state, including any follow-up PUT. */
  flush: () => Promise<void>;
  /** Stop background retries and write the latest dirty state. */
  dispose: () => Promise<void>;
}

/**
 * Gate for store persistence while an opened project's first media hydrate is
 * still in flight. Hydration mutations are not user edits; persisting them
 * produces a spurious PUT storm (historically persisting ephemeral blob URLs
 * into the server document). Starts resumed; the open flow pauses before
 * restoring, resumes once the merged hydrated state has landed.
 */
export interface CanvasPersistPause {
  readonly paused: boolean;
  pause: () => void;
  resume: () => void;
}

export function createCanvasPersistPause(): CanvasPersistPause {
  let paused = false;
  return {
    get paused() {
      return paused;
    },
    pause: () => {
      paused = true;
    },
    resume: () => {
      paused = false;
    },
  };
}

export interface CanvasProjectAutosaveOptions {
  save: () => Promise<void>;
  debounceMs?: number;
  retryBaseMs?: number;
  retryMaxMs?: number;
  onError?: (error: unknown, retryDelayMs: number) => void;
}

/**
 * Create a controller rather than a hook so retry and unload behavior stays
 * deterministic and can be tested without mounting the canvas workspace.
 */
export function createCanvasProjectAutosaveController({
  save,
  debounceMs = CANVAS_AUTOSAVE_DEBOUNCE_MS,
  retryBaseMs = CANVAS_AUTOSAVE_RETRY_BASE_MS,
  retryMaxMs = CANVAS_AUTOSAVE_RETRY_MAX_MS,
  onError,
}: CanvasProjectAutosaveOptions): CanvasProjectAutosaveController {
  let dirty = false;
  let disposed = false;
  let flushAfterInFlight = false;
  let finalFlushRequested = false;
  let consecutiveFailures = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let inFlight: Promise<void> | null = null;

  const clearScheduledSave = () => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const retryDelay = () => canvasAutosaveRetryDelayMs(consecutiveFailures, retryBaseMs, retryMaxMs);

  const schedule = (delay: number) => {
    if (disposed || inFlight) return;
    clearScheduledSave();
    timer = setTimeout(() => {
      timer = null;
      void runSave();
    }, delay);
  };

  const runSave = (): Promise<void> => {
    if (inFlight) return inFlight;
    if (!dirty) return Promise.resolve();

    clearScheduledSave();
    dirty = false;
    const task = (async () => {
      try {
        // Defer invocation one microtask so even a synchronously thrown save
        // cannot clear `inFlight` before this task has been installed.
        await Promise.resolve().then(save);
        consecutiveFailures = 0;
      } catch (error) {
        dirty = true;
        consecutiveFailures += 1;
        onError?.(error, retryDelay());
      } finally {
        inFlight = null;
      }

      if (disposed) {
        // A route change cannot wait indefinitely. If the in-flight request
        // failed or a later edit arrived, give the latest document one final
        // chance to reach disk as part of this same promise.
        if (finalFlushRequested && dirty) {
          finalFlushRequested = false;
          await runSave();
        }
        return;
      }

      if (!dirty) {
        flushAfterInFlight = false;
        return;
      }

      if (flushAfterInFlight) {
        flushAfterInFlight = false;
        await runSave();
        return;
      }

      schedule(consecutiveFailures > 0 ? retryDelay() : debounceMs);
    })();
    inFlight = task;
    return task;
  };

  return {
    markDirty: () => {
      if (disposed) return;
      dirty = true;
      if (!inFlight) schedule(consecutiveFailures > 0 ? retryDelay() : debounceMs);
    },
    flush: () => {
      clearScheduledSave();
      if (inFlight) {
        flushAfterInFlight = true;
        if (disposed) finalFlushRequested = true;
        return inFlight;
      }
      return runSave();
    },
    dispose: () => {
      if (disposed) return inFlight ?? Promise.resolve();
      disposed = true;
      clearScheduledSave();
      finalFlushRequested = true;
      if (inFlight) {
        flushAfterInFlight = true;
        return inFlight;
      }
      if (dirty) return runSave();
      return Promise.resolve();
    },
  };
}
