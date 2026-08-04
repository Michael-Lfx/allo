import { describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import { setBrowserStorageGeneration } from '@/common/utils/browserStorageKey';
import { readInitialMessageDelivery } from '@/renderer/pages/conversation/platforms/initialMessageDelivery';
import { persistGuidInitialMessageHandoff } from './guidInitialMessageHandoff';

const CONVERSATION_ID = parseConversationId('0190f5fe-7c00-7a00-8000-000000000401');

const createStorage = () => {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
};

describe('Guid initial Skill handoff', () => {
  test('persists a source-qualified Skill-only first turn that Nomi and ACP conversations can consume', () => {
    setBrowserStorageGeneration('01900000-0000-7000-8000-000000000401');
    for (const feature of ['initial-message-nomi', 'initial-message-acp'] as const) {
      const storage = createStorage();
      const handoff = persistGuidInitialMessageHandoff({
        storage,
        feature,
        conversationId: CONVERSATION_ID,
        input: '',
        files: [],
        initialSkillIds: ['user:pdf', 'project:workspace:review'],
        idempotencyKey: `guid-skill-only-${feature}`,
      });

      expect(handoff).not.toBeNull();
      expect(handoff?.message).toEqual({
        conversation_id: CONVERSATION_ID,
        initial_admission_epoch: 0,
        input: '',
        inject_skills: ['user:pdf', 'project:workspace:review'],
        idempotency_key: `guid-skill-only-${feature}`,
      });
      expect(readInitialMessageDelivery(storage, handoff?.storageKey ?? '')).toEqual({
        conversation_id: CONVERSATION_ID,
        initial_admission_epoch: 0,
        input: '',
        files: [],
        inject_skills: ['user:pdf', 'project:workspace:review'],
        idempotency_key: `guid-skill-only-${feature}`,
      });
    }
  });

  test('does not create an empty handoff without text or a selected Skill', () => {
    setBrowserStorageGeneration('01900000-0000-7000-8000-000000000401');
    const storage = createStorage();

    const handoff = persistGuidInitialMessageHandoff({
      storage,
      feature: 'initial-message-acp',
      conversationId: CONVERSATION_ID,
      input: '   ',
      files: [],
      initialSkillIds: [],
      idempotencyKey: 'empty-guid-handoff',
    });

    expect(handoff).toBeNull();
    expect(storage.values).toHaveLength(0);
  });
});
