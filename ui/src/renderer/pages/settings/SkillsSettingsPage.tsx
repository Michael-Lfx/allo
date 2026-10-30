/**
 * Skill store — a primary-rail destination.
 * The catalog is the default surface; `?view=installed` shows the local library.
 */
import { Search } from '@icon-park/react';
import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate } from 'react-router-dom';
import useSWR from 'swr';
import NomiInput from '@/renderer/components/base/NomiInput';
import HubPageShell from '@/renderer/components/layout/HubPageShell';
import FilterPills from '@/renderer/pages/videoGeneration/components/FilterPills';
import { CapabilityHubSearchContext, useCapabilityHubSearch } from './capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from './capabilityHub/useCapabilityHubRoute';
import SkillMarketSettings from './SkillMarketSettings';
import SkillsHubSettings from './SkillsHubSettings';
import SkillImportMenu from './skill/SkillImportMenu';
import { AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills } from './skill/availableSkills';
import './components/settings.css';

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
  const { t } = useTranslation();
  const { view, setView, redirectTo } = useCapabilityHubRoute('skills');
  const { data: skills, mutate } = useSWR(AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills);
  const [installedCount, setInstalledCount] = useState(0);
  const [syncedSkills, setSyncedSkills] = useState(skills);
  const [searchQuery, setSearchQuery] = useState('');

  if (skills !== syncedSkills) {
    setSyncedSkills(skills);
    if (skills) setInstalledCount(skills.length);
  }

  const refreshInstalledCount = useCallback(() => {
    void mutate();
  }, [mutate]);

  if (redirectTo) {
    return <Navigate to={redirectTo} replace />;
  }

  const searchPlaceholder = view === 'installed'
    ? t('settings.skillsHub.searchPlaceholder', { defaultValue: '搜索技能...' })
    : t('settings.skillsMarket.searchPlaceholder', { defaultValue: '搜索当前市场技能...' });

  return (
    <CapabilityHubSearchContext.Provider value={{ searchQuery, setSearchQuery }}>
      <HubPageShell
        className='skill-store-page'
        title={t('settings.skillsMarket.title', { defaultValue: '技能' })}
        subtitle={t('settings.skillsMarket.description', {
          defaultValue: '按场景挑选可安装的技能，装好之后可以在对话和设定里直接使用。',
        })}
        maxWidthClass='md:max-w-1240px'
        toolbarClassName='mb-8px'
        toolbar={
          <div className='capability-hub-toolbar' data-testid='skill-store-toolbar'>
            <div className='capability-hub-toolbar-pair'>
              <NomiInput
                allowClear
                height={32}
                value={searchQuery}
                onChange={setSearchQuery}
                data-testid='capability-hub-search'
                className='capability-hub-search w-full'
                placeholder={searchPlaceholder}
                prefix={<Search size={16} fill='currentColor' />}
              />
              <div className='shrink-0' data-testid='capability-hub-installed'>
                <FilterPills
                  ariaLabel={t('settings.skillsMarket.title', { defaultValue: '技能' })}
                  active={view}
                  onChange={setView}
                  items={[
                    { key: 'market', label: t('settings.capabilityHub.discover', { defaultValue: '发现' }) },
                    {
                      key: 'installed',
                      label: `${t('settings.capabilityHub.installed', { defaultValue: '已装' })} ${installedCount}`,
                    },
                  ]}
                />
                <span className='hidden' data-testid='capability-hub-installed-count'>
                  {installedCount}
                </span>
              </div>
              <div className='capability-hub-actions'>
                <SkillImportMenu onImported={refreshInstalledCount} />
              </div>
            </div>
          </div>
        }
      >
        <SkillsHubBody onInstalledCountChange={setInstalledCount} />
      </HubPageShell>
    </CapabilityHubSearchContext.Provider>
  );
};

export default SkillsSettingsPage;
