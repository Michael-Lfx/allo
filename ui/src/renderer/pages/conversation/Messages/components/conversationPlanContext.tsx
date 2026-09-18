/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ConversationId } from '@/common/types/ids';
import { conversationTarget } from '@/common/types/ids';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { dispatchWorkspaceOpenPreviewTool } from '@/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail';
import React, { useCallback, useMemo, useState } from 'react';
import type { PinnedPlanData } from './pinnedPlanModel';

export const CONVERSATION_PLAN_WORKSPACE_TAB = 'conversation-plan';

export type ConversationPlanContextValue = {
  plan: PinnedPlanData | null;
  setPlan: (plan: PinnedPlanData | null) => void;
  canOpenPlanTab: boolean;
  openPlanTab: () => void;
};

const ConversationPlanContext = React.createContext<ConversationPlanContextValue | null>(null);

const noopSetPlan = (_plan: PinnedPlanData | null) => undefined;
const noopOpenPlanTab = () => undefined;

const FALLBACK_VALUE: ConversationPlanContextValue = {
  plan: null,
  setPlan: noopSetPlan,
  canOpenPlanTab: false,
  openPlanTab: noopOpenPlanTab,
};

type ConversationPlanProviderProps = {
  children: React.ReactNode;
  conversationId?: ConversationId;
  workspaceEnabled: boolean;
};

export const ConversationPlanProvider: React.FC<ConversationPlanProviderProps> = ({
  children,
  conversationId,
  workspaceEnabled,
}) => {
  const [plan, setPlan] = useState<PinnedPlanData | null>(null);
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const canOpenPlanTab = Boolean(workspaceEnabled && !isMobile && conversationId);

  const openPlanTab = useCallback(() => {
    if (!conversationId) return;
    dispatchWorkspaceOpenPreviewTool(CONVERSATION_PLAN_WORKSPACE_TAB, conversationTarget(conversationId));
  }, [conversationId]);

  const value = useMemo<ConversationPlanContextValue>(
    () => ({
      plan,
      setPlan,
      canOpenPlanTab,
      openPlanTab,
    }),
    [canOpenPlanTab, openPlanTab, plan]
  );

  return <ConversationPlanContext.Provider value={value}>{children}</ConversationPlanContext.Provider>;
};

export function useConversationPlan(): ConversationPlanContextValue {
  return React.useContext(ConversationPlanContext) ?? FALLBACK_VALUE;
}
