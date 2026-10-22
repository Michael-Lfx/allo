import type { ConversationId } from '@/common/types/ids';
import type { TChatConversation } from '@/common/config/storage';
import { uuid } from '@/common/utils';

export type ConversationVoiceKind = 'unknown' | 'voice' | 'ordinary';

export interface ResolveVoiceKindOptions {
  targetId: ConversationId;
  fetchConversation: (id: ConversationId) => Promise<TChatConversation | null>;
  getCachedConversation?: (id: ConversationId) => TChatConversation | undefined;
}

export async function resolveVoiceConversationKind(
  options: ResolveVoiceKindOptions
): Promise<{ kind: 'voice' | 'ordinary'; workspace?: string }> {
  const { targetId, fetchConversation, getCachedConversation } = options;

  const cached = getCachedConversation?.(targetId);
  if (cached && cached.id === targetId) {
    const isVoice = Boolean(cached.extra && 'voice' in cached.extra && cached.extra.voice === true);
    return {
      kind: isVoice ? 'voice' : 'ordinary',
      workspace: cached.extra?.workspace,
    };
  }

  const res = await fetchConversation(targetId);
  if (!res) {
    throw new Error(`Conversation ${targetId} not found`);
  }
  const isVoice = Boolean(res.extra && 'voice' in res.extra && res.extra.voice === true);
  return {
    kind: isVoice ? 'voice' : 'ordinary',
    workspace: res.extra?.workspace,
  };
}

export interface NomiSendDispatchDependencies {
  conversationId: ConversationId;
  conversationVoiceKind: ConversationVoiceKind;
  workspacePath: string;
  filesToSend: string[];
  message: string;
  voiceSendAttempts: Map<string, string>;
  resolveConversationKind: (conversationId: ConversationId) => Promise<'voice' | 'ordinary'>;
  sendVoiceKeyboardMessage: (conversationId: ConversationId, commandId: string, message: string) => Promise<void>;
  enqueue: (item: { input: string; files: string[]; workspace_path?: string }) => boolean;
  clearFiles: () => void;
  emitFileClear: () => void;
  notifyError: (message: string) => void;
  plainTextOnlyError: string;
  loadFailedError: string;
  generateId?: () => string;
}

export async function dispatchNomiSend(deps: NomiSendDispatchDependencies): Promise<void> {
  let kind = deps.conversationVoiceKind;
  if (kind === 'unknown') {
    try {
      kind = await deps.resolveConversationKind(deps.conversationId);
    } catch (err) {
      deps.notifyError(deps.loadFailedError);
      throw err;
    }
  }

  if (kind === 'voice') {
    if (deps.filesToSend.length > 0) {
      throw new Error(deps.plainTextOnlyError);
    }
    const key = JSON.stringify([deps.conversationId, deps.message]);
    let commandId = deps.voiceSendAttempts.get(key);
    if (!commandId) {
      commandId = (deps.generateId ?? uuid)();
      deps.voiceSendAttempts.set(key, commandId);
    }
    await deps.sendVoiceKeyboardMessage(deps.conversationId, commandId, deps.message);
    if (deps.voiceSendAttempts.get(key) === commandId) {
      deps.voiceSendAttempts.delete(key);
    }
    return;
  }

  const queued = deps.enqueue({
    input: deps.message,
    files: deps.filesToSend,
    workspace_path: deps.workspacePath,
  });
  if (!queued) {
    throw new Error('conversation command was not queued');
  }
  deps.clearFiles();
  deps.emitFileClear();
}
