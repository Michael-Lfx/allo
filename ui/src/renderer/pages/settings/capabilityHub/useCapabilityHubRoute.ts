import { useCallback, useMemo } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { useSettingsNavigationTransition } from '@/renderer/components/layout/SettingsNavigationTransition';
import {
  buildCapabilityHubLocation,
  parseCapabilityHubView,
  resolveLegacyCapabilityLocation,
  type CapabilityHubId,
  type CapabilityHubView,
} from './capabilityHub';

export const useCapabilityHubRoute = (hub: CapabilityHubId) => {
  const location = useLocation();
  const navigate = useNavigate();
  const { navigateWithSettingsTransition } = useSettingsNavigationTransition();
  const [searchParams, setSearchParams] = useSearchParams();
  const inSettings = location.pathname.startsWith('/settings/');
  const redirectTo = resolveLegacyCapabilityLocation(location.pathname, location.search);
  const view = parseCapabilityHubView(searchParams);
  const highlight = searchParams.get('highlight');

  const setView = useCallback(
    (nextView: CapabilityHubView) => {
      const next = new URLSearchParams(searchParams);
      next.delete('tab');
      if (nextView === 'installed') next.set('view', 'installed');
      else next.delete('view');
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams]
  );

  const goToHub = useCallback(
    (nextHub: CapabilityHubId) => {
      const target = buildCapabilityHubLocation({ hub: nextHub, inSettings });
      navigateWithSettingsTransition(target, () => navigate(target));
    },
    [inSettings, navigate, navigateWithSettingsTransition]
  );

  const consumeHighlight = useCallback(() => {
    if (!searchParams.has('highlight')) return;
    const next = new URLSearchParams(searchParams);
    next.delete('highlight');
    setSearchParams(next, { replace: true });
  }, [searchParams, setSearchParams]);

  return useMemo(
    () => ({
      view,
      setView,
      goToHub,
      inSettings,
      highlight,
      consumeHighlight,
      redirectTo,
    }),
    [consumeHighlight, goToHub, highlight, inSettings, redirectTo, setView, view]
  );
};
