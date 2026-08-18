import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ipcBridge } from '@/common';
import type { TChatConversation } from '@/common/config/storage';
import type { ConversationId } from '@/common/types/ids';
import { parseSessionRoute } from '@/renderer/utils/routes/sessionRoute';

export const resolveTitlebarStaticTitleKey = (pathname: string): string | null => {
  if (pathname === '/guid') return null;
  if (pathname.startsWith('/conversation/')) return 'common.titlebar.conversation';
  if (pathname === '/terminal-new' || pathname.startsWith('/terminal/')) return 'common.titlebar.terminal';
  if (pathname.startsWith('/settings')) return 'common.titlebar.settings';
  if (pathname.startsWith('/mcp') || pathname.startsWith('/presets') || pathname.startsWith('/skills') || pathname.startsWith('/plugins')) {
    return 'common.titlebar.capabilityHub';
  }
  if (pathname.startsWith('/video-generation')) return 'common.titlebar.videoGeneration';
  if (pathname.startsWith('/knowledge')) return 'common.titlebar.knowledge';
  if (pathname.startsWith('/learn')) return 'common.titlebar.learning';
  if (pathname.startsWith('/scheduled')) return 'common.titlebar.scheduled';
  if (pathname.startsWith('/requirements')) return 'common.titlebar.workspace';
  if (pathname.startsWith('/models')) return 'common.titlebar.models';
  if (pathname.startsWith('/nomi')) return 'common.titlebar.companion';
  return null;
};

type TitlebarContextTitle = {
  title: string;
  activeConversationId: ConversationId | null;
  conversation: TChatConversation | undefined;
};

export const useTitlebarContextTitle = (pathname: string): TitlebarContextTitle => {
  const { t } = useTranslation();
  const sessionTarget = useMemo(() => parseSessionRoute(pathname), [pathname]);
  const activeConversationId = sessionTarget?.kind === 'conversation' ? sessionTarget.id : null;
  const [conversation, setConversation] = useState<TChatConversation | undefined>();

  useEffect(() => {
    setConversation(undefined);
    if (!activeConversationId) return undefined;

    let cancelled = false;
    const refreshConversation = () => {
      void ipcBridge.conversation.get
        .invoke({ conversation_id: activeConversationId })
        .then((nextConversation) => {
          if (!cancelled) setConversation(nextConversation ?? undefined);
        })
        .catch(() => {
          if (!cancelled) setConversation(undefined);
        });
    };
    refreshConversation();

    const offListChanged = ipcBridge.conversation.listChanged.on((event) => {
      if (event.conversation_id !== activeConversationId) return;
      if (event.action === 'deleted') {
        setConversation(undefined);
        return;
      }
      refreshConversation();
    });

    return () => {
      cancelled = true;
      offListChanged();
    };
  }, [activeConversationId]);

  const staticTitleKey = resolveTitlebarStaticTitleKey(pathname);
  const title = activeConversationId
    ? conversation?.name?.trim() || t('common.titlebar.conversation')
    : staticTitleKey
      ? t(staticTitleKey)
      : '';

  return { title, activeConversationId, conversation };
};
