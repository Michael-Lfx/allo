
import React from 'react';
import { Outlet } from 'react-router-dom';
import { PendingConversationProvider } from './PendingConversationContext';
import PendingConversationOverlay from './PendingConversationOverlay';

/**
 * ConversationShell — the layout route wrapping the session section
 * (`/guid`, `/conversation/:id`, `/terminal-new`, `/terminal/:id`).
 *
 * Conversation routes share the pending-conversation overlay and the matched
 * child route. Session history is owned by the primary sider now, so this
 * shell intentionally contains no secondary-sider construction or listeners.
 */
const ConversationShell: React.FC = () => {
  return (
    <PendingConversationProvider>
      <div className='relative flex size-full min-h-0'>
        {/* 隐藏右侧的会话列表侧边栏，只保留主内容区域 */}
        <div className='relative flex-1 min-w-0 min-h-0 flex flex-col h-full overflow-hidden'>
          <Outlet />
          <PendingConversationOverlay />
        </div>
      </div>
    </PendingConversationProvider>
  );
};

export default ConversationShell;
