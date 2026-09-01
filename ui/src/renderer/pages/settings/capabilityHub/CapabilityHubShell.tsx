import React, { useState } from 'react';
import { Navigate } from 'react-router-dom';
import HubPageShell from '@/renderer/components/layout/HubPageShell';
import CapabilityHubHeader from './CapabilityHubHeader';
import { parseCapabilityHubFromPathname, type CapabilityHubId } from './capabilityHub';
import { useCapabilityHubRoute } from './useCapabilityHubRoute';
import { useSettingsNavigationTransition } from '@/renderer/components/layout/SettingsNavigationTransition';

type CapabilityHubShellProps = {
  hub: CapabilityHubId;
  installedCount?: number;
  extraActions?: React.ReactNode;
  children: React.ReactNode;
};

export const CapabilityHubSearchContext = React.createContext<{
  searchQuery: string;
  setSearchQuery: (value: string) => void;
}>({
  searchQuery: '',
  setSearchQuery: () => undefined,
});

export const useCapabilityHubSearch = () => React.useContext(CapabilityHubSearchContext);

const CapabilityHubShell: React.FC<CapabilityHubShellProps> = ({
  hub,
  installedCount,
  extraActions,
  children,
}) => {
  const { view, setView, goToHub, redirectTo } = useCapabilityHubRoute(hub);
  const { pendingTarget } = useSettingsNavigationTransition();
  const [searchQuery, setSearchQuery] = useState('');
  const pendingHub = pendingTarget
    ? parseCapabilityHubFromPathname(pendingTarget.split(/[?#]/u, 1)[0])
    : null;
  const activeHub = pendingHub ?? hub;

  if (redirectTo) {
    return <Navigate to={redirectTo} replace />;
  }

  return (
    <CapabilityHubSearchContext.Provider value={{ searchQuery, setSearchQuery }}>
      <HubPageShell
        hideHeader
        className='capability-hub-page'
        maxWidthClass='md:max-w-1180px'
        toolbarClassName='mb-8px'
        toolbar={
          <CapabilityHubHeader
            hub={activeHub}
            view={view}
            installedCount={installedCount}
            searchQuery={searchQuery}
            onSearchQueryChange={setSearchQuery}
            onHubChange={(nextHub) => {
              if (nextHub === hub) {
                setView('market');
                return;
              }
              goToHub(nextHub);
            }}
            onToggleInstalled={() => setView(view === 'installed' ? 'market' : 'installed')}
            extraActions={extraActions}
          />
        }
      >
        {children}
      </HubPageShell>
    </CapabilityHubSearchContext.Provider>
  );
};

export default CapabilityHubShell;
