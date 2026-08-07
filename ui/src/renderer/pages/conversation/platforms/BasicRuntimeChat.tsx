/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
import type { ConversationId, CronJobId } from '@/common/types/ids';

import { ConversationProvider } from '@/renderer/hooks/context/ConversationContext';
import FlexFullContainer from '@renderer/components/layout/FlexFullContainer';
import MessageList from '@renderer/pages/conversation/Messages/MessageList';
import {
  MessageListLoadingProvider,
  MessageListProvider,
  useMessageLstCache,
} from '@renderer/pages/conversation/Messages/hooks';
import HOC from '@renderer/utils/ui/HOC';
import React, { useEffect, useMemo } from 'react';
import LocalImageView from '@renderer/components/media/LocalImageView';
import { useConversationResponseMessages } from '@renderer/pages/conversation/Messages/useConversationResponseMessages';
import { BasicRuntimeTurnProvider } from '@renderer/pages/conversation/platforms/BasicRuntimeTurnContext';

export interface BasicRuntimeChatProps {
  conversation_id: ConversationId;
  workspace: string;
  cron_job_id?: CronJobId;
  hideSendBox?: boolean;
  readOnly?: boolean;
  emptySlot?: React.ReactNode;
  loadedSkills?: string[];
}

/**
 * Shared chat surface for the three "basic runtime" platforms
 * (nanobot / remote / openclaw-gateway).
 *
 * These platforms render an identical message list + send box shell; the only
 * platform-specific parts are the ConversationProvider `type` and the send box
 * component, both supplied here. The stateful ACP / Nomi chat surfaces have
 * their own wiring (warmup, pending confirmations, initial message hooks) and
 * are intentionally not built on this factory.
 */
export function createBasicRuntimeChat(
  type: 'nanobot' | 'remote' | 'openclaw-gateway',
  PlatformSendBox: React.ComponentType<{ conversation_id: ConversationId }>
) {
  const BasicRuntimeChat: React.FC<BasicRuntimeChatProps> = ({
    conversation_id,
    workspace,
    cron_job_id,
    hideSendBox,
    readOnly,
    emptySlot,
    loadedSkills,
  }) => {
    const historyPaging = useMessageLstCache(conversation_id, { windowed: true });
    const turnSurface = useConversationResponseMessages(conversation_id, {
      stream: type === 'openclaw-gateway' ? 'openclaw' : 'conversation',
    });
    const updateLocalImage = LocalImageView.useUpdateLocalImage();
    useEffect(() => {
      updateLocalImage({ root: workspace });
    }, [updateLocalImage, workspace]);
    const conversationValue = useMemo(
      () => ({
        conversation_id: conversation_id,
        workspace,
        type,
        cron_job_id,
        hideSendBox,
        readOnly,
        loadedSkills,
        isProcessing: turnSurface.isProcessing,
        activeTurnId: turnSurface.activeTurnId,
        activeRequestMessageId: turnSurface.activeRequestMessageId,
      }),
      [
        conversation_id,
        workspace,
        type,
        cron_job_id,
        hideSendBox,
        readOnly,
        loadedSkills,
        turnSurface.isProcessing,
        turnSurface.activeTurnId,
        turnSurface.activeRequestMessageId,
      ]
    );
    return (
      <BasicRuntimeTurnProvider value={turnSurface}>
        <ConversationProvider value={conversationValue}>
          <div className='flex-1 flex flex-col px-20px min-h-0'>
            <FlexFullContainer>
              <MessageList
                className='flex-1'
                emptySlot={emptySlot}
                onLoadOlder={historyPaging.loadOlder}
                hasMoreOlder={historyPaging.hasMore}
                loadingOlder={historyPaging.loadingOlder}
              />
            </FlexFullContainer>
            {!readOnly && !hideSendBox && <PlatformSendBox conversation_id={conversation_id} />}
          </div>
        </ConversationProvider>
      </BasicRuntimeTurnProvider>
    );
  };
  BasicRuntimeChat.displayName = `BasicRuntimeChat(${type})`;
  return HOC.Wrapper(MessageListProvider, MessageListLoadingProvider, LocalImageView.Provider)(BasicRuntimeChat);
}
