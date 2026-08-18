/**
 * SkillsSettingsPage — skills hub inside the shared capability chrome.
 * Market is the default surface; `?view=installed` shows the local library.
 */
import { ipcBridge } from '@/common';
import React, { useCallback, useEffect, useState } from 'react';
import CapabilityHubShell, { useCapabilityHubSearch } from './capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from './capabilityHub/useCapabilityHubRoute';
import SkillMarketSettings from './SkillMarketSettings';
import SkillsHubSettings from './SkillsHubSettings';
import SkillImportMenu from './skill/SkillImportMenu';

const SkillsHubBody: React.FC<{ onInstalledCountChange: (count: number) => void }> = ({
  onInstalledCountChange,
}) => {
  const { view } = useCapabilityHubRoute('skills');
  const { searchQuery, setSearchQuery } = useCapabilityHubSearch();

  if (view === 'installed') {
    return (
      <SkillsHubSettings
        hideChrome
        searchQuery={searchQuery}
        onSearchQueryChange={setSearchQuery}
        onInstalledCountChange={onInstalledCountChange}
      />
    );
  }

  return (
    <SkillMarketSettings
      hideSearch
      searchQuery={searchQuery}
      onSearchQueryChange={setSearchQuery}
    />
  );
};

const SkillsSettingsPage: React.FC = () => {
  const [installedCount, setInstalledCount] = useState(0);

  const refreshInstalledCount = useCallback(() => {
    void ipcBridge.fs.listAvailableSkills
      .invoke()
      .then((skills) => setInstalledCount(skills.length))
      .catch(() => setInstalledCount(0));
  }, []);

  useEffect(() => {
    refreshInstalledCount();
  }, [refreshInstalledCount]);

  return (
    <CapabilityHubShell
      hub='skills'
      installedCount={installedCount}
      extraActions={<SkillImportMenu onImported={refreshInstalledCount} />}
    >
      <SkillsHubBody onInstalledCountChange={setInstalledCount} />
    </CapabilityHubShell>
  );
};

export default SkillsSettingsPage;
