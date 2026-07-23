
import type { ConversationId, MessageId } from '@/common/types/ids';

import { ipcBridge } from '@/common';
import { transformMessage } from '@/common/chat/chatLib';
import { useCallback, useEffect, useState } from 'react';
import { useAddOrUpdateMessage } from './hooks';

export type ConversationTurnSurface = {
  isProcessing: boolean;
  activeTurnId?: MessageId;
  activeRequestMessageId?: MessageId;
  setAiProcessing: (running: boolean) => void;
};

type StreamKind = 'conversation' | 'openclaw';

/**
 * Owns message rendering + shared turn surface state for conversation runtimes
 * whose composer only needs local busy tracking. Chat shells should expose
 * `isProcessing` / `activeTurnId` through ConversationProvider so MessageList
 * disclosure stays consistent across ACP / OpenClaw / Nanobot / Remote.
 */
export function useConversationResponseMessages(
  conversation_id: ConversationId,
  options?: { stream?: StreamKind }
): ConversationTurnSurface {
  const addOrUpdateMessage = useAddOrUpdateMessage();
  const streamKind = options?.stream ?? 'conversation';
  const [isProcessing, setIsProcessing] = useState(false);
  const [activeTurnId, setActiveTurnId] = useState<MessageId | undefined>();
  const [activeRequestMessageId, setActiveRequestMessageId] = useState<MessageId | undefined>();

  const setAiProcessing = useCallback((running: boolean) => {
    setIsProcessing(running);
    if (!running) {
      setActiveTurnId(undefined);
      setActiveRequestMessageId(undefined);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void ipcBridge.conversation.get
      .invoke({ id: conversation_id })
      .then((conversation) => {
        if (cancelled || !conversation?.runtime) return;
        setIsProcessing(conversation.runtime.is_processing === true);
      })
      .catch(() => {
        /* best-effort hydrate */
      });
    return () => {
      cancelled = true;
    };
  }, [conversation_id]);

  useEffect(() => {
    const stream =
      streamKind === 'openclaw'
        ? ipcBridge.openclawConversation.responseStream
        : ipcBridge.conversation.responseStream;

    return stream.on((message) => {
      if (message.conversation_id !== conversation_id) {
        return;
      }

      if (message.type === 'thought') {
        setIsProcessing(true);
        return;
      }

      if (message.type === 'finish') {
        setIsProcessing(false);
        setActiveTurnId(undefined);
        setActiveRequestMessageId(undefined);
        return;
      }

      if (message.type === 'error') {
        setIsProcessing(false);
        setActiveTurnId(undefined);
        setActiveRequestMessageId(undefined);
        const transformedMessage = transformMessage(message);
        if (transformedMessage) {
          addOrUpdateMessage(transformedMessage);
        }
        return;
      }

      const transformedMessage = transformMessage(message);
      if (transformedMessage) {
        addOrUpdateMessage(transformedMessage);
        if (transformedMessage.type === 'text' && transformedMessage.position === 'right') {
          setActiveRequestMessageId((prev) => prev ?? (transformedMessage.msg_id as MessageId | undefined));
        }
      }

      if (message.type === 'content' || message.type === 'acp_permission' || message.type === 'tool_call') {
        setIsProcessing(true);
        const turnId = (message as { turn_id?: MessageId }).turn_id ?? (message.msg_id as MessageId | undefined);
        if (turnId) {
          setActiveTurnId((prev) => prev ?? turnId);
        }
      }
    });
  }, [addOrUpdateMessage, conversation_id, streamKind]);

  return {
    isProcessing,
    activeTurnId,
    activeRequestMessageId,
    setAiProcessing,
  };
}
