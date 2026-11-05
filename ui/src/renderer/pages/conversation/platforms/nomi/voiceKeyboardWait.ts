import type { TMessage } from '@/common/chat/chatLib';
import type { ConversationId } from '@/common/types/ids';
import type { VoiceTaskSnapshot } from '@/common/adapter/voiceCompanion';

export type VoiceKeyboardWaitPhase =
  | 'sending'
  | 'queued'
  | 'accepted'
  | 'delivering'
  | 'cancelling'
  | 'syncing'
  | 'unknown'
  | 'failed';

export interface VoiceKeyboardWait {
  conversationId: ConversationId;
  commandId: string;
  text: string;
  phase: VoiceKeyboardWaitPhase;
  startedAt: number;
  task?: VoiceTaskSnapshot | null;
}

export const VOICE_KEYBOARD_WAIT_MAX_MS = 108_000;

export function isVoiceKeyboardWaitExpired(
  wait: Pick<VoiceKeyboardWait, 'startedAt'>,
  now: number
): boolean {
  return now - wait.startedAt >= VOICE_KEYBOARD_WAIT_MAX_MS;
}

export const voiceKeyboardWaitStatusKey = (phase: VoiceKeyboardWaitPhase): string => {
  switch (phase) {
    case 'sending': return 'voice.keyboardWait.sending';
    case 'queued': return 'voice.keyboardWait.queued';
    case 'accepted': return 'voice.keyboardWait.accepted';
    case 'delivering': return 'voice.keyboardWait.delivering';
    case 'cancelling': return 'voice.keyboardWait.cancelling';
    case 'syncing': return 'voice.keyboardWait.syncing';
    case 'unknown': return 'voice.keyboardWait.unknown';
    case 'failed': return 'voice.keyboardWait.failed';
    default: {
      const exhaustive: never = phase;
      return exhaustive;
    }
  }
};

const UNKNOWN_DELIVERY_CODES = new Set([
  'task_delivery_unknown',
  'task_delivery_pending',
  'task_creation_unknown',
  'task_status_unavailable',
]);

/** Delivery errors that do not prove the command failed keep the same id under observation. */
export function phaseFromDeliveryError(error: unknown): VoiceKeyboardWaitPhase {
  const message = error instanceof Error ? error.message : String(error ?? '');
  if (UNKNOWN_DELIVERY_CODES.has(message) || /fetch|network|timeout|timed out|abort/i.test(message)) {
    return 'unknown';
  }
  return 'failed';
}

export function phaseFromVoiceTask(task: VoiceTaskSnapshot | null | undefined): VoiceKeyboardWaitPhase {
  switch (task?.status) {
    case 'queued': return 'queued';
    case 'accepted': return 'accepted';
    case 'delivering': return 'delivering';
    case 'completed': return 'syncing';
    case 'failed': return 'failed';
    case 'cancelling': return 'cancelling';
    case 'unknown':
    case undefined:
    default: return 'unknown';
  }
}

function matchingUserIndex(messages: readonly TMessage[], task: VoiceTaskSnapshot | null | undefined): number {
  const ids = [task?.message_id, task?.runtime_turn_id].filter((id): id is string => Boolean(id));
  if (ids.length === 0) return -1;
  return messages.findIndex(message => {
    const rowIds = [message.message_id, message.msg_id, message.turn_id].filter(
      (id): id is NonNullable<typeof id> => Boolean(id)
    );
    return rowIds.some(id => ids.includes(id));
  });
}

/** True once an authoritative user row and a following left reply exist. */
export function hasAuthoritativeVoiceReply(
  messages: readonly TMessage[],
  task: VoiceTaskSnapshot | null | undefined
): boolean {
  const userIndex = matchingUserIndex(messages, task);
  if (userIndex < 0) return false;
  return messages.slice(userIndex + 1).some(message => message.position === 'left' && message.type === 'text');
}

/**
 * The right-side user preview is the only local message that belongs in the
 * transcript. The left wait row is rendered separately (VoiceKeyboardWaitRow)
 * so MessageList cannot fold it into a turn/process disclosure.
 */
export function buildVoiceKeyboardWaitUserPreview(
  wait: VoiceKeyboardWait | null | undefined,
  persisted: readonly TMessage[],
  now = Date.now()
): TMessage[] {
  if (!wait) return [];
  if (hasAuthoritativeVoiceReply(persisted, wait.task)) return [];
  const userAlreadyPersisted = persisted.some(message => {
    if (message.type !== 'text' || message.position !== 'right') return false;
    if (
      wait.task?.message_id != null &&
      (message.message_id === wait.task.message_id || message.msg_id === wait.task.message_id)
    ) {
      return true;
    }
    const createdAt = message.created_at ?? 0;
    return (
      message.content.content.trim() === wait.text.trim() &&
      createdAt >= wait.startedAt - 1_000
    );
  });
  if (userAlreadyPersisted) return [];
  return [{
    id: `voice-keyboard-wait:${wait.commandId}:user`,
    type: 'text',
    position: 'right',
    conversation_id: wait.conversationId,
    status: 'pending',
    created_at: wait.startedAt || now,
    content: { content: wait.text },
  }];
}
