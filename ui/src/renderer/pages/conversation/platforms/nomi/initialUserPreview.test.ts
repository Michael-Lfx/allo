import { describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import { setBrowserStorageGeneration } from '@/common/utils/browserStorageKey';
import { persistGuidInitialMessageHandoff } from '@/renderer/pages/guid/hooks/guidInitialMessageHandoff';
import {
  initialMessageMustWaitForModelCatalog,
  nomiInitialUserPreview,
  readNomiInitialHandoff,
} from './initialUserPreview';

const conversationId = parseConversationId('0190f5fe-7c00-7a00-8000-000000000401');

const memoryStorage = () => {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
};

describe('nomi initial user preview', () => {
  test('shows the unsent home message as a user bubble and leaves the handoff in place', () => {
    setBrowserStorageGeneration('01900000-0000-7000-8000-000000000401');
    const storage = memoryStorage();
    persistGuidInitialMessageHandoff({
      storage,
      feature: 'initial-message-nomi',
      conversationId,
      input: '  你是什么专家  ',
      files: [],
      initialSkillIds: [],
      idempotencyKey: 'idem-1',
    });

    const preview = nomiInitialUserPreview(storage, conversationId);

    expect(preview?.position).toBe('right');
    expect(preview?.content.content).toBe('你是什么专家');
    expect(readNomiInitialHandoff(storage, conversationId)?.idempotency_key).toBe('idem-1');
  });

  test('text handoffs do not wait for the model catalog, image handoffs do', () => {
    expect(initialMessageMustWaitForModelCatalog(undefined)).toBe(false);
    expect(initialMessageMustWaitForModelCatalog([])).toBe(false);
    expect(initialMessageMustWaitForModelCatalog(['C:/shot.png'])).toBe(true);
  });

  test('does not invent a bubble when this conversation has no home handoff', () => {
    setBrowserStorageGeneration('01900000-0000-7000-8000-000000000401');
    expect(nomiInitialUserPreview(memoryStorage(), conversationId)).toBeNull();
  });
});
