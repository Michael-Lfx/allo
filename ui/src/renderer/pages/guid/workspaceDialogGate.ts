/**
 * Shares the pending native workspace picker across every homepage entry point.
 * A prompt selection can request a workspace while the deferred send check is
 * still observing the preceding, workspace-less state.
 */
export const createWorkspaceDialogGate = (): ((task: () => Promise<void>) => Promise<void>) => {
  let inFlight: Promise<void> | null = null;

  return (task) => {
    if (inFlight) return inFlight;

    inFlight = task().finally(() => {
      inFlight = null;
    });
    return inFlight;
  };
};
