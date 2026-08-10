import type {
  EditResubmitLifecycleEvent,
  EditResubmitResolution,
} from '@/renderer/components/chat/SendBox/editResubmitTypes';

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
