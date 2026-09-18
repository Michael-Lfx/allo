import type { ConversationId } from '@/common/types/ids';

import { useEffect } from 'react';
import { useLocation } from 'react-router-dom';
import { useConversationHistoryContext } from '@renderer/hooks/context/ConversationHistoryContext';
import { parseSessionRoute } from '@renderer/utils/routes/sessionRoute';

/** Route-derived conversation target, or `null` for every other route. */
export const resolveActiveConversationId = (pathname: string): ConversationId | null => {
  const route = parseSessionRoute(pathname);
  return route?.kind === 'conversation' ? route.id : null;
};

/**
 * Keeps the active-conversation bookkeeping correct even when the session
 * sidebar is unmounted (collapsed sidebar, settings route, mobile drawer).
 * The sidebar can be hidden at any time, so the route-derived target must be
 * written by the persistent layout instead of only by sidebar mount effects.
 */
export const useActiveConversationRouteSync = (): void => {
  const { setActiveConversation } = useConversationHistoryContext();
  const { pathname } = useLocation();

  useEffect(() => {
    setActiveConversation(resolveActiveConversationId(pathname));
  }, [pathname, setActiveConversation]);
};
