import React, { useRef } from 'react';
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

const McpHubBody: React.FC<{
  panelRef: React.RefObject<McpInstalledPanelHandle | null>;
  mcpMessage: React.ComponentProps<typeof ToolsModalContentWithState>['mcpMessage'];
  mcpMessageContext: React.ReactNode;
  mcpServers: ReturnType<typeof useMcpServers>['mcpServers'];
  extensionMcpServers: ReturnType<typeof useMcpServers>['extensionMcpServers'];
  saveMcpServers: ReturnType<typeof useMcpServers>['saveMcpServers'];
  setMcpServers: ReturnType<typeof useMcpServers>['setMcpServers'];
  addedStateLoading: boolean;
}> = ({
  panelRef,
  mcpMessage,
  mcpMessageContext,
  mcpServers,
  extensionMcpServers,
  saveMcpServers,
  setMcpServers,
  addedStateLoading,
}) => {
  const { view } = useCapabilityHubRoute('mcp');
  const { searchQuery, setSearchQuery } = useCapabilityHubSearch();

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
        hideChrome
        searchQuery={searchQuery}
        showList={view === 'installed'}
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
    isMcpServersLoading,
    mcpServersLoadFailed,
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
        addedStateLoading={isMcpServersLoading || mcpServersLoadFailed}
      />
    </CapabilityHubShell>
  );
};

export default McpPage;
