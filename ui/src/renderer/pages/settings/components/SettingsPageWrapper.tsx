import classNames from 'classnames';
import React from 'react';
import { filterDeveloperGatedTabs } from '@/common/config/developerMode';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { SettingsViewModeProvider } from '@/renderer/components/settings/SettingsModal/settingsViewContext';
import { resolveExtensionAssetUrl } from '@/renderer/utils/platform';
import { type IExtensionSettingsTab } from '@/common/adapter/ipcBridge';
import { useExtensionSettingsTabs } from '@/renderer/hooks/system/useExtensionSettingsTabs';
import { Computer, Earth, Info, Pic, Brain, ChartPie, CloudStorage, Puzzle, Robot, System, Tool, TreeDiagram } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { useExtI18n } from '@/renderer/hooks/system/useExtI18n';
import { BUILTIN_TAB_IDS } from './SettingsSider';
import { buildSettingsNavItems } from './settingsNavigation';
import './settings.css';

interface SettingsPageWrapperProps {
  children: React.ReactNode;
  className?: string;
  contentClassName?: string;
}

type NavItem = { label: string; icon: React.ReactElement; path: string; id: string };

type TranslateFn = (key: string, options?: { defaultValue?: string }) => string;

export function getBuiltinSettingsNavItems(
  t: TranslateFn,
  options?: { developerModeEnabled?: boolean }
): NavItem[] {
  const builtinMap: Record<string, NavItem> = {
    system: { id: 'system', label: t('settings.system'), icon: <System theme='outline' size='16' />, path: 'system' },
    'browser-use': {
      id: 'browser-use',
      label: t('settings.browserUseNav'),
      icon: <Earth theme='outline' size='16' />,
      path: 'browser-use',
    },
    'computer-use': {
      id: 'computer-use',
      label: t('settings.computerUseNav'),
      icon: <Computer theme='outline' size='16' />,
      path: 'computer-use',
    },
    poi: {
      id: 'poi',
      label: t('settings.poiNav'),
      icon: <Brain theme='outline' size='16' />,
      path: 'poi',
    },
    insights: {
      id: 'insights',
      label: t('settings.insightsNav'),
      icon: <ChartPie theme='outline' size='16' />,
      path: 'insights',
    },
    moa: {
      id: 'moa',
      label: t('settings.moaNav'),
      icon: <TreeDiagram theme='outline' size='16' />,
      path: 'moa',
    },
    media: {
      id: 'media',
      label: t('settings.mediaNav'),
      icon: <Pic theme='outline' size='16' />,
      path: 'media',
    },
    presets: {
      id: 'presets',
      label: t('settings.presetsHub.railTitle', { defaultValue: '预设' }),
      icon: <Robot theme='outline' size='16' />,
      path: 'presets',
    },
    skills: {
      id: 'skills',
      label: t('settings.skillsHub.railTitle', { defaultValue: '技能' }),
      icon: <Puzzle theme='outline' size='16' />,
      path: 'skills',
    },
    mcp: {
      id: 'mcp',
      label: t('settings.mcpHub.railTitle', { defaultValue: 'MCP' }),
      icon: <Tool theme='outline' size='16' />,
      path: 'mcp',
    },
    'cloud-login': {
      id: 'cloud-login',
      label: t('settings.cloudLoginNav'),
      icon: <CloudStorage theme='outline' size='16' />,
      path: 'cloud-login',
    },
    about: { id: 'about', label: t('settings.about'), icon: <Info theme='outline' size='16' />, path: 'about' },
  };

  const visibleBuiltinTabIds = filterDeveloperGatedTabs(
    BUILTIN_TAB_IDS,
    options?.developerModeEnabled === true
  );

  return visibleBuiltinTabIds.map((id) => builtinMap[id]);
}

const SettingsPageWrapper: React.FC<SettingsPageWrapperProps> = ({ children, className, contentClassName }) => {
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { t } = useTranslation();

  const extensionTabs = useExtensionSettingsTabs();

  const { resolveExtTabName } = useExtI18n();
  const [developerMode] = useConfig('system.developerMode');

  const menuItems = React.useMemo(() => {
    const builtins = getBuiltinSettingsNavItems(t, { developerModeEnabled: developerMode === true });

    const toNavItem = (tab: IExtensionSettingsTab): NavItem => {
      const resolvedIcon = resolveExtensionAssetUrl(tab.icon) || tab.icon;
      return {
        id: tab.id,
        label: resolveExtTabName(tab),
        icon: resolvedIcon ? (
          <img src={resolvedIcon} alt='' className='w-16px h-16px object-contain' />
        ) : (
          <Puzzle theme='outline' size='16' />
        ),
        path: `ext/${tab.id}`,
      };
    };

    // Insert extension tabs at their anchor, or (unanchored) at the end of the
    // "Application" group — before "about" — to keep them inside that group.
    return buildSettingsNavItems(builtins, extensionTabs, toNavItem).items;
  }, [t, extensionTabs, resolveExtTabName, developerMode]);

  const containerClass = classNames(
    'app-page-shell settings-page-wrapper w-full min-h-full box-border overflow-y-auto',
    className
  );

  const contentClass = classNames('settings-page-content mx-auto w-full md:max-w-1024px', contentClassName);

  return (
    <SettingsViewModeProvider value='page'>
      <div className={containerClass}>
        {isMobile && (
          <div className='settings-mobile-top-nav'>
            {menuItems.map((item) => {
              const active = pathname.includes(`/settings/${item.path}`);
              return (
                <button
                  key={item.path}
                  type='button'
                  className={classNames('settings-mobile-top-nav__item', {
                    'settings-mobile-top-nav__item--active': active,
                  })}
                  onClick={() => {
                    // Absolute paths (e.g. /nomi) navigate directly; relative paths are settings sub-routes
                    const target = item.path.startsWith('/') ? item.path : `/settings/${item.path}`;
                    void navigate(target, { replace: true });
                  }}
                >
                  <span className='settings-mobile-top-nav__icon'>{item.icon}</span>
                  <span className='settings-mobile-top-nav__label'>{item.label}</span>
                </button>
              );
            })}
          </div>
        )}
        <div className={contentClass}>{children}</div>
      </div>
    </SettingsViewModeProvider>
  );
};

export default SettingsPageWrapper;
