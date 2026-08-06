import { useCallback, useMemo } from 'react';
import { configService } from '@/common/config/configService';
import type { ProviderId } from '@/common/types/ids';
import { useConfig } from '@/renderer/hooks/config/useConfig';

export type ConversationTitleModelChoice = { provider_id: ProviderId; model: string } | null;

const STORAGE_KEY = 'conversation.titleModel';

/**
 * Persisted preferred model for conversation auto-title generation. `null`
 * (absent setting) lets the backend fall back to the conversation's own
 * session model. Mirrors the knowledge `useKnowledgeAutogenModel` pattern.
 */
export function useConversationTitleModel() {
  // Read reactively (useSyncExternalStore subscription) so the selector
  // reflects writes immediately.
  const [stored] = useConfig(STORAGE_KEY);

  const choice = useMemo<ConversationTitleModelChoice>(() => {
    if (!stored?.provider_id || !stored.model) return null;
    return { provider_id: stored.provider_id, model: stored.model };
  }, [stored?.provider_id, stored?.model]);

  const setChoice = useCallback(async (next: ConversationTitleModelChoice) => {
    if (next) {
      await configService.set(STORAGE_KEY, { provider_id: next.provider_id, model: next.model });
    } else {
      await configService.remove(STORAGE_KEY);
    }
  }, []);

  return { choice, setChoice };
}
