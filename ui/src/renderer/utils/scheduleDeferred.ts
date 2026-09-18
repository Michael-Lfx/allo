/**
 * Schedule non-critical startup work after first paint settles.
 * Prefer idle time; always honor a minimum delay so OTA/network work
 * does not compete with the initial route render.
 */
export function scheduleDeferred(
  callback: () => void,
  options?: { minDelayMs?: number; idleTimeoutMs?: number }
): () => void {
  const minDelayMs = options?.minDelayMs ?? 2_500;
  const idleTimeoutMs = options?.idleTimeoutMs ?? 5_000;
  let cancelled = false;
  let idleHandle: number | null = null;
  let timerHandle: number | null = null;

  const run = () => {
    if (cancelled) return;
    callback();
  };

  const runWhenIdle = () => {
    if (cancelled) return;
    const idleWindow = window as Window & {
      requestIdleCallback?: (cb: () => void, opts?: { timeout?: number }) => number;
      cancelIdleCallback?: (id: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      idleHandle = idleWindow.requestIdleCallback(run, { timeout: idleTimeoutMs });
      return;
    }
    timerHandle = window.setTimeout(run, 0);
  };

  timerHandle = window.setTimeout(runWhenIdle, minDelayMs);

  return () => {
    cancelled = true;
    if (timerHandle != null) window.clearTimeout(timerHandle);
    const idleWindow = window as Window & {
      cancelIdleCallback?: (id: number) => void;
    };
    if (idleHandle != null) idleWindow.cancelIdleCallback?.(idleHandle);
  };
}
