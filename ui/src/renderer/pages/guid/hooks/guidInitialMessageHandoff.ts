import { conversationTarget, type ConversationId } from '@/common/types/ids';
import { sessionStorageKey } from '@/common/utils/browserStorageKey';
import { hasGuidInitialPayload } from './autoWorkEntry';

type InitialMessageStorage = Pick<Storage, 'setItem'>;

export type GuidInitialMessageFeature =
  | 'initial-message-nomi'
  | 'initial-message-acp'
  | 'initial-message-remote';

export type GuidInitialMessage = {
  conversation_id: ConversationId;
  initial_admission_epoch: 0;
  input: string;
  files?: string[];
  inject_skills?: string[];
  idempotency_key: string;
};

export type GuidInitialMessageHandoff = {
  storageKey: string;
  message: GuidInitialMessage;
};

type PersistGuidInitialMessageHandoffParams = {
  storage: InitialMessageStorage;
  feature: GuidInitialMessageFeature;
  conversationId: ConversationId;
  input: string;
  files: readonly string[];
  initialSkillIds: readonly string[];
  idempotencyKey: string;
};

/**
 * Persist the Guid page's normal first turn before navigation. A selected
 * source-qualified Skill is itself a valid first-turn payload, even when the
 * user has not supplied prose yet.
 */
export function persistGuidInitialMessageHandoff({
  storage,
  feature,
  conversationId,
  input,
  files,
  initialSkillIds,
  idempotencyKey,
}: PersistGuidInitialMessageHandoffParams): GuidInitialMessageHandoff | null {
  if (!hasGuidInitialPayload(input, initialSkillIds)) return null;

  const message: GuidInitialMessage = {
    conversation_id: conversationId,
    initial_admission_epoch: 0,
    input,
    ...(files.length > 0 ? { files: [...files] } : {}),
    ...(initialSkillIds.length > 0 ? { inject_skills: [...initialSkillIds] } : {}),
    idempotency_key: idempotencyKey,
  };
  const storageKey = sessionStorageKey(feature, conversationTarget(conversationId));
  storage.setItem(storageKey, JSON.stringify(message));
  return { storageKey, message };
}
