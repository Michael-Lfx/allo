import type { ConversationId } from '@/common/types/ids';

export interface SingleFlightRef<T> {
  current: Promise<T> | null;
}

export const runSingleFlight = <T>(
  ref: SingleFlightRef<T>,
  operation: () => Promise<T>
): Promise<T> => {
  if (ref.current) return ref.current;
  const promise = operation().finally(() => {
    if (ref.current === promise) ref.current = null;
  });
  ref.current = promise;
  return promise;
};

type MaybePromise = void | Promise<void>;

/** Production reset lifecycle shared by the button handler and behavior test. */
export const runConversationResetSingleFlight = ({
  inFlightRef,
  conversationId,
  invokeReset,
  onStart,
  onSuccess,
  onError,
  onSettled,
}: {
  inFlightRef: SingleFlightRef<void>;
  conversationId: ConversationId;
  invokeReset: (params: { conversation_id: ConversationId }) => Promise<unknown>;
  onStart: () => MaybePromise;
  onSuccess: () => MaybePromise;
  onError: (error: unknown) => MaybePromise;
  onSettled: () => MaybePromise;
}): Promise<void> =>
  runSingleFlight(inFlightRef, async () => {
    await onStart();
    try {
      await invokeReset({ conversation_id: conversationId });
      await onSuccess();
    } catch (error) {
      await onError(error);
    } finally {
      await onSettled();
    }
  });
