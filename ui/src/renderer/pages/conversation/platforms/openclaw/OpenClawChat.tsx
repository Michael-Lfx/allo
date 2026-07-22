/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
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
import { usePendingConfirmationsRecovery } from '@renderer/pages/conversation/Messages/usePendingConfirmationsRecovery';
import { useConversationResponseMessages } from '@renderer/pages/conversation/Messages/useConversationResponseMessages';
import HOC from '@renderer/utils/ui/HOC';
import React, { useEffect } from 'react';
import LocalImageView from '@renderer/components/media/LocalImageView';
import OpenClawSendBox from './OpenClawSendBox';

const OpenClawChat: React.FC<{
  conversation_id: ConversationId;
  workspace: string;
  cron_job_id?: CronJobId;
  hideSendBox?: boolean;
  readOnly?: boolean;
  emptySlot?: React.ReactNode;
  loadedSkills?: string[];
}> = ({ conversation_id, workspace, cron_job_id, hideSendBox, readOnly, emptySlot, loadedSkills }) => {
  const historyPaging = useMessageLstCache(conversation_id, { windowed: true });
  usePendingConfirmationsRecovery(conversation_id, { enabled: !readOnly });
  const turnSurface = useConversationResponseMessages(conversation_id, { stream: 'openclaw' });
  const updateLocalImage = LocalImageView.useUpdateLocalImage();
  useEffect(() => {
    updateLocalImage({ root: workspace });
  }, [updateLocalImage, workspace]);
  return (
    <ConversationProvider
      value={{
        conversation_id: conversation_id,
        workspace,
        type: 'openclaw-gateway',
        cron_job_id,
        hideSendBox,
        readOnly,
        isProcessing: turnSurface.isProcessing,
        activeTurnId: turnSurface.activeTurnId,
        activeRequestMessageId: turnSurface.activeRequestMessageId,
        loadedSkills,
      }}
    >
      <div className='flex-1 flex flex-col px-20px min-h-0'>
        <FlexFullContainer>
          <MessageList
            className='flex-1'
            emptySlot={emptySlot}
            onLoadOlder={historyPaging.loadOlder}
            hasMoreOlder={historyPaging.hasMore}
            loadingOlder={historyPaging.loadingOlder}
          ></MessageList>
        </FlexFullContainer>
        {!readOnly && !hideSendBox && <OpenClawSendBox conversation_id={conversation_id} />}
      </div>
    </ConversationProvider>
  );
};

export default HOC.Wrapper(MessageListProvider, MessageListLoadingProvider, LocalImageView.Provider)(OpenClawChat);
