/**
 * Process-lifecycle core for run-dev.mjs.
 *
 * The real entry point supplies child-process creation and termination. Keeping
 * the race/state machine here makes Vite/Tauri failure behavior testable
 * without starting a desktop window or a real dev server.
 */

export function waitForExit(child) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };
    child.once('error', (error) => finish({ code: null, error }));
    child.once('close', (code, signal) => finish({ code, signal }));
  });
}

export function createSupervisor({
  startVite,
  startTauri,
  waitForVite,
  waitForExitImpl = waitForExit,
  stopProcess,
  log = () => {},
  now = () => Date.now(),
  maxRestarts = 5,
  restartWindowMs = 5 * 60_000,
}) {
  let viteProcess;
  let tauriLaunch;
  let tauriProcess;
  let viteExit;
  let tauriExit;
  let stopping = false;
  let cleanupPromise;
  const restartTimes = [];
  const viteState = { settled: false, result: undefined };

  const trackViteExit = (child) => {
    viteExit = waitForExitImpl(child).then((result) => {
      viteState.settled = true;
      viteState.result = result;
      return result;
    });
    return viteExit;
  };

  const stopAll = async (reason = 'user-interrupt') => {
    if (cleanupPromise) return cleanupPromise;
    stopping = true;
    cleanupPromise = (async () => {
      const launch = tauriLaunch;
      await Promise.all([
        tauriProcess ? stopProcess(tauriProcess, reason) : undefined,
        viteProcess ? stopProcess(viteProcess, reason) : undefined,
      ].filter(Boolean));
      await Promise.allSettled([tauriExit, viteExit].filter(Boolean));
      try {
        await launch?.cleanup?.();
      } catch (error) {
        log('[run-dev] failed to clean the Tauri restart signal:', error);
      }
    })();
    return cleanupPromise;
  };

  const run = async () => {
    viteProcess = startVite();
    trackViteExit(viteProcess);

    try {
      const startup = await Promise.race([
        Promise.resolve().then(() => waitForVite(viteProcess)),
        viteExit.then((result) => ({ viteExited: true, result })),
      ]);
      if (
        startup?.viteExited ||
        viteState.settled ||
        (viteProcess?.exitCode !== null && viteProcess?.exitCode !== undefined)
      ) {
        const result = startup?.result ?? viteState.result;
        throw new Error(
          `Vite exited before becoming healthy (code ${result?.code ?? 'unknown'})`,
        );
      }
    } catch (error) {
      const requestedStop = stopping;
      await stopAll('supervisor-error');
      if (requestedStop) return 0;
      throw error;
    }

    while (!stopping) {
      tauriLaunch = startTauri();
      tauriProcess = tauriLaunch?.child ?? tauriLaunch;
      tauriExit = waitForExitImpl(tauriProcess);
      const event = await Promise.race([
        tauriExit.then((result) => ({ kind: 'tauri', result })),
        viteExit.then((result) => ({ kind: 'vite', result })),
      ]);

      if (event.kind === 'vite') {
        const launch = tauriLaunch;
        if (stopping) {
          await stopProcess(tauriProcess, 'user-interrupt');
          await tauriExit;
          await launch?.cleanup?.();
          return 0;
        }
        await stopProcess(tauriProcess, 'vite-failed');
        await tauriExit;
        tauriProcess = undefined;
        tauriLaunch = undefined;
        await launch?.cleanup?.();
        throw new Error(
          `Vite exited while Tauri was running (code ${event.result.code ?? 'unknown'})`,
        );
      }

      const result = event.result;
      const launch = tauriLaunch;
      let restartRequested = false;
      if (!stopping) {
        try {
          restartRequested = Boolean(await launch?.consumeRestartSignal?.());
        } catch (error) {
          log('[run-dev] failed to consume the Tauri restart signal:', error);
        }
      }
      try {
        await launch?.cleanup?.();
      } catch (error) {
        log('[run-dev] failed to clean the Tauri restart signal:', error);
      }
      tauriProcess = undefined;
      tauriLaunch = undefined;
      if (stopping) return 0;
      if (viteState.settled) {
        throw new Error(
          `Vite exited while Tauri was stopping (code ${viteState.result?.code ?? 'unknown'})`,
        );
      }
      if (result.error) throw result.error;

      if (restartRequested) {
        const currentTime = now();
        restartTimes.push(currentTime);
        while (restartTimes.length && currentTime - restartTimes[0] > restartWindowMs) {
          restartTimes.shift();
        }
        if (restartTimes.length > maxRestarts) {
          await stopAll('supervisor-error');
          throw new Error(
            `development restart circuit breaker opened after ${restartTimes.length} restarts ` +
              `in ${restartWindowMs}ms (exit code ${result.code ?? 'unknown'})`,
          );
        }
        log('[run-dev] Tauri requested a development restart; keeping Vite alive');
        continue;
      }

      // A normal Tauri exit owns the supervisor shutdown. Stop Vite and wait
      // for both children before returning the Tauri exit code.
      await stopAll('supervisor-error');
      return result.code ?? 1;
    }
    return 0;
  };

  return {
    run,
    stop: stopAll,
    get stopping() {
      return stopping;
    },
  };
}
