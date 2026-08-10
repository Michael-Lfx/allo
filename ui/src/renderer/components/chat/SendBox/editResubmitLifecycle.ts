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
  (authoritative || activeOperationId === eventOperationId);

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
