import classNames from 'classnames';
import React from 'react';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { SettingsViewModeProvider } from '@/renderer/components/settings/SettingsModal/settingsViewContext';
import {
  BookOpen,
  Brain,
  ChartPie,
  CloudStorage,
  Computer,
  Earth,
  Info,
  Pic,
  Puzzle,
  Robot,
  System,
  Tool,
  TreeDiagram,
} from '@icon-park/react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { type SettingsNavIcon, type SettingsNavItem, useSettingsNavigation } from './settingsNavigation';
import './settings.css';

interface SettingsPageWrapperProps {
  children: React.ReactNode;
  className?: string;
  contentClassName?: string;
  layout?: 'form' | 'hub';
}

const iconByName: Record<Exclude<SettingsNavIcon, 'extension'>, React.ComponentType<any>> = {
  system: System,
  'browser-use': Earth,
  'computer-use': Computer,
  poi: Brain,
  learning: BookOpen,
  insights: ChartPie,
  moa: TreeDiagram,
  media: Pic,
  presets: Robot,
  skills: Puzzle,
  mcp: Tool,
  'cloud-login': CloudStorage,
  about: Info,
};

const isActivePath = (pathname: string, path: string): boolean => {
  const target = path.startsWith('/') ? path : `/settings/${path}`;
  return pathname === target || pathname.startsWith(`${target}/`);
};

const MobileNavIcon: React.FC<{ item: SettingsNavItem }> = ({ item }) => {
  if (item.icon === 'extension' && item.iconUrl) {
    return <img src={item.iconUrl} alt='' className='size-16px object-contain' />;
  }
  const Icon = item.icon === 'extension' ? Puzzle : iconByName[item.icon];
  return <Icon theme='outline' size='16' />;
};

const SettingsPageWrapper: React.FC<SettingsPageWrapperProps> = ({ children, className, contentClassName, layout: layoutMode = 'form' }) => {
  const { t } = useTranslation();
  const layoutContext = useLayoutContext();
  const isMobile = layoutContext?.isMobile ?? false;
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { groups } = useSettingsNavigation();
  const menuItems = React.useMemo(() => groups.flatMap((group) => group.items), [groups]);

  const containerClass = classNames(
    'app-page-shell settings-page-wrapper w-full min-h-full box-border overflow-y-auto',
    className
  );
  const contentClass = classNames(
    'settings-page-content mx-auto w-full',
    layoutMode === 'hub' ? 'settings-page-content--hub' : 'settings-page-content--form',
    contentClassName
  );

  return (
    <SettingsViewModeProvider value='page'>
      <div className={containerClass}>
        {isMobile && (
          <nav className='settings-mobile-top-nav' aria-label={t('settings.title')}>
            {menuItems.map((item) => {
              const active = isActivePath(pathname, item.path);
              const target = item.path.startsWith('/') ? item.path : `/settings/${item.path}`;
              return (
                <button
                  key={item.id}
                  type='button'
                  aria-current={active ? 'page' : undefined}
                  className={classNames('settings-mobile-top-nav__item', {
                    'settings-mobile-top-nav__item--active': active,
                  })}
                  onClick={() => void navigate(target, { replace: true })}
                >
                  <span className='settings-mobile-top-nav__icon'>
                    <MobileNavIcon item={item} />
                  </span>
                  <span className='settings-mobile-top-nav__label'>{item.label}</span>
                </button>
              );
            })}
          </nav>
        )}
        <main className={contentClass} data-settings-page-content>{children}</main>
      </div>
    </SettingsViewModeProvider>
  );
};

export default SettingsPageWrapper;
