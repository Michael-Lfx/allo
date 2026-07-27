
import type { IKnowledgeBinding } from '@/common/adapter/ipcBridge';
import type { KnowledgeBaseId } from '@/common/types/ids';
import { defaultKnowledgeBinding } from '@/renderer/pages/conversation/components/KnowledgeControl';

const ACTIVATION_KEY = 'flowy:knowledge-activation:v1';

export type KnowledgeActivationPayload = {
  knowledge_base_id: KnowledgeBaseId;
  suggest_prompt: string;
  binding: IKnowledgeBinding;
};

export function stashKnowledgeActivation(payload: KnowledgeActivationPayload): void {
  try {
    sessionStorage.setItem(ACTIVATION_KEY, JSON.stringify(payload));
  } catch {
    // ignore quota / private mode
  }
}

export function consumeKnowledgeActivation(): KnowledgeActivationPayload | null {
  try {
    const raw = sessionStorage.getItem(ACTIVATION_KEY);
    if (!raw) return null;
    sessionStorage.removeItem(ACTIVATION_KEY);
    const parsed = JSON.parse(raw) as KnowledgeActivationPayload;
    if (!parsed?.knowledge_base_id || !parsed?.suggest_prompt) return null;
    return {
      knowledge_base_id: parsed.knowledge_base_id,
      suggest_prompt: parsed.suggest_prompt,
      binding: {
        ...defaultKnowledgeBinding(),
        ...parsed.binding,
        enabled: true,
        kb_ids: parsed.binding?.kb_ids?.length
          ? parsed.binding.kb_ids
          : [parsed.knowledge_base_id],
      },
    };
  } catch {
    return null;
  }
}

export function bindingForNewBase(knowledgeBaseId: KnowledgeBaseId): IKnowledgeBinding {
  return {
    ...defaultKnowledgeBinding(),
    enabled: true,
    kb_ids: [knowledgeBaseId],
  };
}
