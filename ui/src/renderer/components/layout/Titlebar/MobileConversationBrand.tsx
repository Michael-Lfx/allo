
import type { TChatConversation } from '@/common/config/storage';
import { AgentLogoIcon } from '@/renderer/components/agent/AgentBadge';
import { usePresetInfo } from '@/renderer/hooks/agent/usePresetInfo';
import React from 'react';

type MobileConversationBrandProps = {
  conversation: TChatConversation | undefined;
  fallbackTitle: string;
};

const MobileConversationBrand: React.FC<MobileConversationBrandProps> = ({ conversation, fallbackTitle }) => {
  const { info: preset } = usePresetInfo(conversation || undefined);

  const backend =
    conversation?.type === 'acp'
      ? conversation.extra?.backend
      : conversation?.type === 'nomi'
        ? 'nomi'
        : conversation?.type === 'openclaw-gateway'
            ? 'openclaw-gateway'
            : conversation?.type === 'nanobot'
              ? 'nanobot'
              : conversation?.type === 'remote'
                ? 'remote'
                : undefined;

  const showLogo = Boolean(backend || preset);
  const title = conversation?.name || fallbackTitle;

  return (
    <span className='app-titlebar__brand-mobile'>
      {showLogo && (
        <AgentLogoIcon
          backend={backend}
          agent_name={title}
          agentLogo={preset?.logo}
          agentLogoIsEmoji={preset?.isEmoji}
        />
      )}
      <span className='app-titlebar__brand-text'>{title}</span>
    </span>
  );
};

export default MobileConversationBrand;
