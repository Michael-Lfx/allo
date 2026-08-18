import { ipcBridge } from '@/common';
import React, { useCallback, useEffect, useState } from 'react';
import CapabilityHubShell, {
  useCapabilityHubSearch,
} from '@/renderer/pages/settings/capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from '@/renderer/pages/settings/capabilityHub/useCapabilityHubRoute';
import PluginSettingsPanel from './PluginSettingsPanel';

const PluginHubBody: React.FC<{ onInstalledCountChange: (count: number) => void }> = ({
  onInstalledCountChange,
}) => {
  const { view } = useCapabilityHubRoute('plugins');
  const { searchQuery, setSearchQuery } = useCapabilityHubSearch();

  return (
    <PluginSettingsPanel
      section={view === 'installed' ? 'installed' : 'market'}
      hideChrome
      hideSearch
      searchQuery={searchQuery}
      onSearchQueryChange={setSearchQuery}
      onInstalledCountChange={onInstalledCountChange}
    />
  );
};

const PluginSettingsPage: React.FC = () => {
  const [installedCount, setInstalledCount] = useState(0);

  const refreshCount = useCallback(() => {
    void ipcBridge.extensions.getLoadedExtensions
      .invoke()
      .then((extensions) => setInstalledCount(extensions.length))
      .catch(() => setInstalledCount(0));
  }, []);

  useEffect(() => {
    refreshCount();
  }, [refreshCount]);

  return (
    <CapabilityHubShell hub='plugins' installedCount={installedCount}>
      <PluginHubBody onInstalledCountChange={setInstalledCount} />
    </CapabilityHubShell>
  );
};

export default PluginSettingsPage;
