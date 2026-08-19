/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * The single declaration site for the right-rail mini-app entry.
 *
 * Currently unmounted from conversation `extraTabs` (the cube shortcut is hidden
 * on the rail, matching the left home nav). Keep this descriptor in one place so
 * a later remount cannot let two layouts drift apart in label, glyph or position.
 */
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { ApplicationOne } from '@icon-park/react';
import MiniAppPanel from '@/renderer/pages/conversation/Workspace/MiniAppPanel';
import type { WorkspaceExtraTab } from '@/renderer/pages/conversation/Workspace/types';

export const CONVERSATION_MINIAPP_TAB_KEY = 'conversation-miniapps';

export function useConversationMiniAppTab(): WorkspaceExtraTab {
  const { t } = useTranslation();
  return useMemo(
    () => ({
      key: CONVERSATION_MINIAPP_TAB_KEY,
      title: t('miniApps.nav.entry'),
      icon: <ApplicationOne size={18} />,
      content: <MiniAppPanel />,
    }),
    [t]
  );
}
