import type { ISendMessageResult } from '@/common/adapter/ipcBridge';

export type PublicMessageDeliveryDisposition =
  | 'fresh'
  | 'replayed_in_flight'
  | 'replayed_completed';

/**
 * Classify the server's durable delivery receipt.
 *
 * A replay is an acknowledgement of an existing operation, never authority to
 * declare a new local turn. In-flight replays are reconciled from the
 * authoritative conversation GET; completed replays remain closed and only
 * refresh persisted history.
 */
export const classifyPublicMessageDelivery = (
  delivery: Pick<ISendMessageResult, 'replayed' | 'completed'>
): PublicMessageDeliveryDisposition => {
  if (!delivery.replayed) return 'fresh';
  return delivery.completed ? 'replayed_completed' : 'replayed_in_flight';
};

export const shouldDeclareFreshTurn = (
  delivery: Pick<ISendMessageResult, 'replayed' | 'completed'>
): boolean => classifyPublicMessageDelivery(delivery) === 'fresh';

/**
 * Only a fresh, non-empty request may create a visible user text row. Empty
 * input is still allowed for Skill/file-only runtime work, but has no bubble.
 */
export const shouldRenderFreshUserMessage = (
  delivery: Pick<ISendMessageResult, 'replayed' | 'completed'>,
  displayMessage: string
): boolean => shouldDeclareFreshTurn(delivery) && displayMessage.trim().length > 0;
