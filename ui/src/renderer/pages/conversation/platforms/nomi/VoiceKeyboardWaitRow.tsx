import type { IMessageThinking } from '@/common/chat/chatLib';
import MessageThinking from '@renderer/pages/conversation/Messages/components/MessageThinking';
import React, { useMemo } from 'react';
import type { VoiceKeyboardWait } from './voiceKeyboardWait';

/**
 * A view-local left wait row. It deliberately lives outside MessageList's
 * turn/process pipeline: the keyboard task is a host task without a runtime
 * turn id, so letting MessageList classify it as process content freezes its
 * timer once the surrounding disclosure is closed. MessageThinking's
 * standalone variant keeps the same visual language but owns a live clock.
 */
const VoiceKeyboardWaitRow: React.FC<{
  wait: VoiceKeyboardWait;
  statusText: string;
}> = ({ wait, statusText }) => {
  const message = useMemo<IMessageThinking>(
    () => ({
      id: `voice-keyboard-wait:${wait.commandId}:left`,
      type: 'thinking',
      position: 'left',
      conversation_id: wait.conversationId,
      status: 'pending',
      created_at: wait.startedAt,
      content: {
        content: '',
        subject: statusText,
        status: 'thinking',
      },
    }),
    [wait.commandId, wait.conversationId, wait.startedAt, statusText]
  );

  return (
    <div
      className='shrink-0 w-full max-w-780px mx-auto px-8px m-t-10px'
      data-testid='voice-keyboard-wait-row'
      data-command-id={wait.commandId}
      role='status'
      aria-live='polite'
    >
      <MessageThinking message={message} />
    </div>
  );
};

export default VoiceKeyboardWaitRow;
