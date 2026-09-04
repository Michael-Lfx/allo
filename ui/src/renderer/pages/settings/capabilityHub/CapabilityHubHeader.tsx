import { Puzzle, Robot, Search, Tool, ApplicationOne } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import NomiInput from '@/renderer/components/base/NomiInput';
import type { I18nKey } from '@/renderer/services/i18n';
import { CAPABILITY_HUB_IDS, type CapabilityHubId, type CapabilityHubView } from './capabilityHub';
import '../components/settings.css';

const TAB_ICONS: Record<CapabilityHubId, typeof Robot> = {
  presets: Robot,
  skills: Puzzle,
  mcp: Tool,
  plugins: ApplicationOne,
};

const TAB_LABEL_KEYS: Record<CapabilityHubId, I18nKey> = {
  presets: 'settings.presetsHub.railTitle',
  skills: 'settings.skillsHub.railTitle',
  mcp: 'settings.mcpHub.railTitle',
  plugins: 'settings.capabilityHub.tabPlugins',
};

const SEARCH_PLACEHOLDER_KEYS: Record<CapabilityHubId, I18nKey> = {
  presets: 'settings.capabilityHub.searchPresets',
  skills: 'settings.capabilityHub.searchSkills',
  mcp: 'settings.capabilityHub.searchMcp',
  plugins: 'settings.capabilityHub.searchPlugins',
};

const TAB_LABEL_DEFAULTS: Record<CapabilityHubId, string> = {
  presets: 'Presets',
  skills: 'Skills',
  mcp: 'MCP',
  plugins: 'Plugins',
};

const SEARCH_PLACEHOLDER_DEFAULTS: Record<CapabilityHubId, string> = {
  presets: 'Search presets',
  skills: 'Search skills',
  mcp: 'Search MCP',
  plugins: 'Search plugins',
};

type CapabilityHubHeaderProps = {
  hub: CapabilityHubId;
  marketEnabled?: boolean;
  view: CapabilityHubView;
  installedCount?: number;
  searchQuery: string;
  onSearchQueryChange: (value: string) => void;
  onHubChange: (hub: CapabilityHubId) => void;
  onToggleInstalled: () => void;
  extraActions?: React.ReactNode;
};

const CapabilityHubHeader: React.FC<CapabilityHubHeaderProps> = ({
  hub,
  marketEnabled = true,
  view,
  installedCount,
  searchQuery,
  onSearchQueryChange,
  onHubChange,
  onToggleInstalled,
  extraActions,
}) => {
  const { t } = useTranslation();

  return (
    <div className='capability-hub-header' data-testid='capability-hub-header'>
      <div className='capability-hub-tabs-row'>
        <div className='capability-hub-tabs' role='tablist' aria-label={t('settings.groupCapabilityExtensions')}>
          {CAPABILITY_HUB_IDS.map((id) => {
            const Icon = TAB_ICONS[id];
            const selected = id === hub;
            return (
              <button
                key={id}
                type='button'
                role='tab'
                aria-selected={selected}
                data-testid={`capability-hub-tab-${id}`}
                className={classNames('capability-hub-tab', { 'capability-hub-tab--active': selected })}
                onClick={() => onHubChange(id)}
              >
                <Icon theme='outline' size={16} fill='currentColor' />
                <span>{t(TAB_LABEL_KEYS[id], { defaultValue: TAB_LABEL_DEFAULTS[id] })}</span>
              </button>
            );
          })}
        </div>
        {extraActions ? <div className='capability-hub-actions'>{extraActions}</div> : null}
      </div>

      <div className='capability-hub-toolbar'>
        <div className='capability-hub-toolbar-pair'>
          <NomiInput
            allowClear
            height={36}
            value={searchQuery}
            onChange={onSearchQueryChange}
            data-testid='capability-hub-search'
            className='capability-hub-search w-full'
            placeholder={t(SEARCH_PLACEHOLDER_KEYS[hub], { defaultValue: SEARCH_PLACEHOLDER_DEFAULTS[hub] })}
            prefix={<Search size={16} fill='currentColor' />}
          />

          <div className='capability-hub-segment' role='group' aria-label={t('settings.capabilityHub.navLabel')}>
            {marketEnabled && (
              <button
                type='button'
                className={classNames('capability-hub-segment-btn', {
                  'capability-hub-segment-btn--active': view === 'market',
                })}
                aria-pressed={view === 'market'}
                onClick={() => {
                  if (view === 'installed') onToggleInstalled();
                }}
              >
                {t('settings.capabilityHub.discover', { defaultValue: 'Discover' })}
              </button>
            )}
            <button
              type='button'
              className={classNames('capability-hub-segment-btn', {
                'capability-hub-segment-btn--active': view === 'installed',
              })}
              aria-pressed={view === 'installed'}
              data-testid='capability-hub-installed'
              onClick={() => {
                if (view === 'market') onToggleInstalled();
              }}
            >
              <span>{t('settings.capabilityHub.installed', { defaultValue: 'Installed' })}</span>
              {typeof installedCount === 'number' && (
                <span className='capability-hub-count' data-testid='capability-hub-installed-count'>
                  {installedCount}
                </span>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CapabilityHubHeader;
