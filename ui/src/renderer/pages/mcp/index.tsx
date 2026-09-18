import React, { useCallback, useEffect, useRef } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useMcpServers } from '@/renderer/hooks/mcp';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import {
  ToolsModalContentWithState,
  type McpInstalledPanelHandle,
} from '@/renderer/components/settings/SettingsModal/contents/ToolsModalContent';
import CapabilityHubShell, { useCapabilityHubSearch } from '@/renderer/pages/settings/capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from '@/renderer/pages/settings/capabilityHub/useCapabilityHubRoute';
import McpAddServerButton from './McpAddServerButton';
import McpMarketSettings from './McpMarketSettings';
import type { McpActivationNavigationState } from '@/renderer/hooks/mcp/useMcpActivationFlow';

/** One-shot navigation state handed over by the market "add and enable" flow. */
export type McpInstalledNavState = {
  mcpFocusIds?: string[];
  activation?: McpActivationNavigationState;
};

const McpHubBody: React.FC<{
  panelRef: React.RefObject<McpInstalledPanelHandle | null>;
  mcpMessage: React.ComponentProps<typeof ToolsModalContentWithState>['mcpMessage'];
  mcpMessageContext: React.ReactNode;
  mcpServers: ReturnType<typeof useMcpServers>['mcpServers'];
  extensionMcpServers: ReturnType<typeof useMcpServers>['extensionMcpServers'];
  saveMcpServers: ReturnType<typeof useMcpServers>['saveMcpServers'];
  setMcpServers: ReturnType<typeof useMcpServers>['setMcpServers'];
  isMcpServersLoading: ReturnType<typeof useMcpServers>['isUserMcpServersLoading'];
  mcpServersLoadFailed: ReturnType<typeof useMcpServers>['userMcpServersLoadFailed'];
  reloadMcpServers: ReturnType<typeof useMcpServers>['reloadMcpServers'];
  addedStateLoading: boolean;
}> = ({
  panelRef,
  mcpMessage,
  mcpMessageContext,
  mcpServers,
  extensionMcpServers,
  saveMcpServers,
  setMcpServers,
  isMcpServersLoading,
  mcpServersLoadFailed,
  reloadMcpServers,
  addedStateLoading,
}) => {
  const { view } = useCapabilityHubRoute('mcp');
  const { searchQuery, setSearchQuery } = useCapabilityHubSearch();
  const location = useLocation();
  const navigate = useNavigate();
  const navState = (location.state ?? null) as McpInstalledNavState | null;

  // Snapshot the one-shot navigation state before stripping it from history so
  // back/forward or re-mounts cannot re-trigger the auto activation.
  const pendingRef = useRef<McpInstalledNavState | null | undefined>(undefined);
  const navStateKey = navState?.activation?.operationId ?? navState?.mcpFocusIds?.join('|') ?? null;
  const pendingStateKey = pendingRef.current?.activation?.operationId ?? pendingRef.current?.mcpFocusIds?.join('|') ?? null;
  if (navState && navStateKey !== pendingStateKey) {
    pendingRef.current = navState;
  }

  useEffect(() => {
    if (!navState) return;
    navigate(`${location.pathname}${location.search}`, { replace: true, state: null });
  }, [location.pathname, location.search, navigate, navState]);

  const clearPendingNavState = useCallback(() => {
    pendingRef.current = null;
  }, []);

  return (
    <>
      <ToolsModalContentWithState
        ref={panelRef}
        mcpMessage={mcpMessage}
        mcpMessageContext={mcpMessageContext}
        mcpServers={mcpServers}
        extensionMcpServers={extensionMcpServers}
        saveMcpServers={saveMcpServers}
        setMcpServers={setMcpServers}
        isMcpServersLoading={isMcpServersLoading}
        mcpServersLoadFailed={mcpServersLoadFailed}
        reloadMcpServers={reloadMcpServers}
        hideChrome
        searchQuery={searchQuery}
        showList={view === 'installed'}
        pendingActivation={
          pendingRef.current?.activation
        }
        pendingFocusIds={pendingRef.current?.mcpFocusIds}
        onPendingConsumed={clearPendingNavState}
      />
      {view !== 'installed' && (
        <McpMarketSettings
          saveMcpServers={saveMcpServers}
          mcpServers={mcpServers}
          addedStateLoading={addedStateLoading}
          hideSearch
          searchQuery={searchQuery}
          onSearchQueryChange={setSearchQuery}
        />
      )}
    </>
  );
};

const McpPage: React.FC = () => {
  const [mcpMessage, mcpMessageContext] = useArcoMessage({ maxCount: 10 });
  const panelRef = useRef<McpInstalledPanelHandle>(null);
  const {
    mcpServers,
    extensionMcpServers,
    isUserMcpServersLoading,
    userMcpServersLoadFailed,
    reloadMcpServers,
    saveMcpServers,
    setMcpServers,
  } = useMcpServers();

  return (
    <CapabilityHubShell
      hub='mcp'
      installedCount={mcpServers.length + extensionMcpServers.length}
      extraActions={<McpAddServerButton onOpen={(mode) => panelRef.current?.openAdd(mode)} />}
    >
      <McpHubBody
        panelRef={panelRef}
        mcpMessage={mcpMessage}
        mcpMessageContext={mcpMessageContext}
        mcpServers={mcpServers}
        extensionMcpServers={extensionMcpServers}
        saveMcpServers={saveMcpServers}
        setMcpServers={setMcpServers}
        isMcpServersLoading={isUserMcpServersLoading}
        mcpServersLoadFailed={userMcpServersLoadFailed}
        reloadMcpServers={reloadMcpServers}
        addedStateLoading={isUserMcpServersLoading}
      />
    </CapabilityHubShell>
  );
};

export default McpPage;
