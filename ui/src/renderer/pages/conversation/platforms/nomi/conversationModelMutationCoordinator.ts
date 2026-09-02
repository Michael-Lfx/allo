/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type ExplicitModelSelectionTicket = {
  requestId: number;
};

export type ConversationModelMutationCoordinator = ReturnType<
  typeof createConversationModelMutationCoordinator
>;

/**
 * Serializes all conversation model/pool mutations that can be initiated by
 * the mounted Nomi panel. Explicit user selections invalidate queued automatic
 * reconciliation, while an automatic mutation already in flight is allowed to
 * finish before the user's queued mutation writes the final state.
 */
export const createConversationModelMutationCoordinator = () => {
  let queue: Promise<unknown> = Promise.resolve();
  let mutationVersion = 0;
  let nextRequestId = 0;
  let latestRequestId = 0;
  let explicitSelectionInFlight = false;

  const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
    const next = queue.then(operation, operation);
    queue = next.then(
      () => undefined,
      () => undefined,
    );
    return next;
  };

  const beginExplicitSelection = (): ExplicitModelSelectionTicket => {
    const requestId = ++nextRequestId;
    latestRequestId = requestId;
    mutationVersion += 1;
    explicitSelectionInFlight = true;
    return { requestId };
  };

  const completeExplicitSelection = (requestId: number) => {
    if (requestId === latestRequestId) {
      explicitSelectionInFlight = false;
    }
  };

  return {
    enqueue,
    beginExplicitSelection,
    completeExplicitSelection,
    isLatestExplicitSelection: (requestId: number) => requestId === latestRequestId,
    isExplicitSelectionInFlight: () => explicitSelectionInFlight,
    currentMutationVersion: () => mutationVersion,
    canRunAutomaticMutation: (observedVersion: number) =>
      !explicitSelectionInFlight && observedVersion === mutationVersion,
    markAutomaticMutationApplied: () => {
      mutationVersion += 1;
    },
  };
};
