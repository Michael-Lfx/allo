/**
 * SkillsSettingsPage — skills hub inside the shared capability chrome.
 * Market is the default surface; `?view=installed` shows the local library.
 */
import React, { useCallback, useEffect, useState } from 'react';
import useSWR from 'swr';
import CapabilityHubShell, { useCapabilityHubSearch } from './capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from './capabilityHub/useCapabilityHubRoute';
import SkillMarketSettings from './SkillMarketSettings';
import SkillsHubSettings from './SkillsHubSettings';
import SkillImportMenu from './skill/SkillImportMenu';
import { AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills } from './skill/availableSkills';

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
  const { data: skills, mutate } = useSWR(AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills);
  const [installedCount, setInstalledCount] = useState(0);

  useEffect(() => {
    if (skills) setInstalledCount(skills.length);
  }, [skills]);

  const refreshInstalledCount = useCallback(() => {
    void mutate();
  }, [mutate]);

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
