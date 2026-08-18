import type {
  EditResubmitLifecycleEvent,
  EditResubmitResolution,
} from '@/renderer/components/chat/SendBox/editResubmitTypes';

export interface ComposerDraftRevisionState {
  current: number;
  input: string;
}

export const createComposerDraftRevisionState = (input: string): ComposerDraftRevisionState => ({
  current: 0,
  input,
});

/** Advance synchronously at the input boundary; repeated observation is a no-op. */
export const recordComposerDraftChange = (
  state: ComposerDraftRevisionState,
  nextInput: string
): number => {
  if (state.input !== nextInput) {
    state.input = nextInput;
    state.current += 1;
  }
  return state.current;
};

export const commitComposerDraftChange = (
  state: ComposerDraftRevisionState,
  nextInput: string,
  commit: (nextInput: string) => void
): void => {
  recordComposerDraftChange(state, nextInput);
  commit(nextInput);
};

export type EditResubmitComposerClearActions = {
  tokenInput?: { clear: () => unknown };
  commitEmptyDraft: () => void;
  clearDomSnippets: () => void;
  clearReplyQuote: () => void;
};

/**
 * Clear every transient composer surface after edit admission. The token input
 * owns the canonical draft when mounted; the controlled fallback keeps the
 * revision ledger correct for hosts that render a plain input instead.
 */
export const clearEditResubmitComposer = ({
  tokenInput,
  commitEmptyDraft,
  clearDomSnippets,
  clearReplyQuote,
}: EditResubmitComposerClearActions): void => {
  if (tokenInput) {
    tokenInput.clear();
  } else {
    commitEmptyDraft();
  }
  clearDomSnippets();
  clearReplyQuote();
};

interface CommitEditResubmitTerminalOptions {
  event: Extract<EditResubmitLifecycleEvent, { kind: 'terminal' }>;
  publish?: (event: EditResubmitLifecycleEvent) => void;
  onPublishError: (error: unknown) => void;
  afterPublish?: (published: boolean) => void;
  clearSharedState: () => void;
  releaseOperation: () => void;
}

export const shouldCommitEditResubmitTerminal = (
  activeOperationId: string | null,
  committedOperationIds: ReadonlySet<string>,
  eventOperationId: string,
  authoritative: boolean
): boolean =>
  !committedOperationIds.has(eventOperationId) &&
  (activeOperationId === eventOperationId || (activeOperationId === null && authoritative));

/** Restore a retry only when no terminal or durable owner has already handled it. */
export const shouldRestoreRetrySubmittedInput = ({
  activeOperationId,
  committedOperationIds,
  eventOperationId,
  durableOperationId,
  revisionUnchanged,
}: {
  activeOperationId: string | null;
  committedOperationIds: ReadonlySet<string>;
  eventOperationId: string;
  durableOperationId?: string;
  revisionUnchanged: boolean;
}): boolean =>
  activeOperationId === eventOperationId &&
  !committedOperationIds.has(eventOperationId) &&
  durableOperationId === undefined &&
  revisionUnchanged;

/** Keep terminal tombstones bounded while preventing late authoritative replays. */
export const rememberEditResubmitOperation = (
  committedOperationIds: Set<string>,
  operationId: string,
  maxEntries = 256
): void => {
  committedOperationIds.add(operationId);
  while (committedOperationIds.size > maxEntries) {
    const oldest = committedOperationIds.values().next().value;
    if (typeof oldest !== 'string') return;
    committedOperationIds.delete(oldest);
  }
};

/** Publish the renderer terminal first, then retire shared operation ownership. */
export const commitEditResubmitTerminal = ({
  event,
  publish,
  onPublishError,
  afterPublish,
  clearSharedState,
  releaseOperation,
}: CommitEditResubmitTerminalOptions): EditResubmitResolution => {
  let published = false;
  if (publish) {
    try {
      publish(event);
      published = true;
    } catch (error) {
      onPublishError(error);
    }
  }
  try {
    afterPublish?.(published);
  } finally {
    try {
      clearSharedState();
    } finally {
      releaseOperation();
    }
  }
  return event.resolution;
};
