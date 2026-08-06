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
}) {
  let viteProcess;
  let tauriLaunch;
  let tauriProcess;
  let viteExit;
  let tauriExit;
  let stopping = false;
  let cleanupPromise;
  const viteState = { settled: false, result: undefined };

  const trackViteExit = (child) => {
    viteExit = waitForExitImpl(child).then((result) => {
      viteState.settled = true;
      viteState.result = result;
      return result;
    });
    return viteExit;
  };

  const stopAll = async () => {
    if (cleanupPromise) return cleanupPromise;
    stopping = true;
    cleanupPromise = (async () => {
      const launch = tauriLaunch;
      if (tauriProcess) stopProcess(tauriProcess);
      if (viteProcess) stopProcess(viteProcess);
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
        Promise.resolve().then(() => waitForVite()),
        viteExit.then((result) => ({ viteExited: true, result })),
      ]);
      if (startup?.viteExited) {
        throw new Error(
          `Vite exited before becoming healthy (code ${startup.result.code ?? 'unknown'})`,
        );
      }
    } catch (error) {
      const requestedStop = stopping;
      await stopAll();
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
          stopProcess(tauriProcess);
          await tauriExit;
          await launch?.cleanup?.();
          return 0;
        }
        stopProcess(tauriProcess);
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
        log('[run-dev] Tauri requested a development restart; keeping Vite alive');
        continue;
      }

      // A normal Tauri exit owns the supervisor shutdown. Stop Vite and wait
      // for both children before returning the Tauri exit code.
      await stopAll();
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
