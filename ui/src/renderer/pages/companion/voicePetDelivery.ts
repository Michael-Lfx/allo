import {parseConversationId} from '@common/types/ids';
import {claimCompanionTurnDelivery, completeCompanionTurnDelivery, persistCompanionTurnDelivery, readCompanionTurnDelivery, releaseCompanionTurnDelivery} from './companionTurnDelivery';

type Ports = {
  getTask: (commandId: string) => Promise<{conversation_id?: string | null}>;
  send: (conversationId: string, commandId: string, text: string) => Promise<unknown>;
};

/** Resolve an uncertain prior command before sending again with its original identity. */
export async function deliverVoicePetText(storage: Pick<Storage,'getItem'|'setItem'|'removeItem'>, key: string, conversationId: string, text: string, ports: Ports) {
  if (!claimCompanionTurnDelivery(key)) return null;
  try {
    const existing = readCompanionTurnDelivery(storage,key);
    const delivery = existing ?? persistCompanionTurnDelivery(storage,key,parseConversationId(conversationId),text.trim(),[]);
    if (delivery.conversation_id !== conversationId) throw new Error('task_scope_changed');
    let accepted = false;
    if (existing) {
      try {
        const task = await ports.getTask(delivery.idempotency_key);
        if (task.conversation_id !== conversationId) throw new Error('task_scope_changed');
        accepted = true;
      } catch (error) {
        if (!(error instanceof Error) || error.message !== 'task_not_found') throw error;
      }
    }
    if (!accepted) await ports.send(delivery.conversation_id,delivery.idempotency_key,delivery.input);
    completeCompanionTurnDelivery(storage,key,delivery.conversation_id,delivery.idempotency_key);
    return delivery;
  } finally { releaseCompanionTurnDelivery(key); }
}
