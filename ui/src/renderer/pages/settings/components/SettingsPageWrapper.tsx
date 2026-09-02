import classNames from 'classnames';
import React, { useLayoutEffect } from 'react';
import SettingsContentLoading from '@/renderer/components/layout/SettingsContentLoading';
import { useSettingsNavigationTransition } from '@/renderer/components/layout/SettingsNavigationTransition';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { SettingsViewModeProvider } from '@/renderer/components/settings/SettingsModal/settingsViewContext';
import {
  BookOpen,
  Brain,
  ChartHistogram,
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
import {
  isSettingsNavItemActive,
  type SettingsNavIcon,
  type SettingsNavItem,
  useSettingsNavigation,
} from './settingsNavigation';
import './settings.css';

interface SettingsPageWrapperProps {
  children: React.ReactNode;
  className?: string;
  contentClassName?: string;
  layout?: 'form' | 'hub';
  /** Initial data loading state. Refreshes with existing data should stay inline. */
  loading?: boolean;
}

const iconByName: Record<Exclude<SettingsNavIcon, 'extension'>, React.ComponentType<any>> = {
  system: System,
  'browser-use': Earth,
  'computer-use': Computer,
  poi: Brain,
  learning: BookOpen,
  insights: ChartPie,
  telemetry: ChartHistogram,
  moa: TreeDiagram,
  media: Pic,
  presets: Robot,
  skills: Puzzle,
  mcp: Tool,
  'cloud-login': CloudStorage,
  about: Info,
};

const MobileNavIcon: React.FC<{ item: SettingsNavItem }> = ({ item }) => {
  if (item.icon === 'extension' && item.iconUrl) {
    return <img src={item.iconUrl} alt='' className='size-16px object-contain' />;
  }
  const Icon = item.icon === 'extension' ? Puzzle : iconByName[item.icon];
  return <Icon theme='outline' size='16' />;
};

const SettingsPageWrapper: React.FC<SettingsPageWrapperProps> = ({
  children,
  className,
  contentClassName,
  layout: layoutMode = 'form',
  loading = false,
}) => {
  const { t } = useTranslation();
  const layoutContext = useLayoutContext();
  const isMobile = layoutContext?.isMobile ?? false;
  const navigate = useNavigate();
  const { navigateWithSettingsTransition, markSettingsNavigationReady, pendingTarget } =
    useSettingsNavigationTransition();
  const { pathname } = useLocation();
  const navigationPathname = pendingTarget?.split(/[?#]/u, 1)[0] || pathname;
  const { groups } = useSettingsNavigation();
  const menuItems = React.useMemo(() => groups.flatMap((group) => group.items), [groups]);

  useLayoutEffect(() => {
    if (!loading) markSettingsNavigationReady();
  }, [loading, markSettingsNavigationReady]);

  const containerClass = classNames(
    'app-page-shell settings-page-wrapper w-full min-h-0 flex-1 box-border overflow-y-auto',
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
              const active = isSettingsNavItemActive(navigationPathname, item);
              const target = item.path.startsWith('/') ? item.path : `/settings/${item.path}`;
              return (
                <button
                  key={item.id}
                  type='button'
                  aria-current={active ? 'page' : undefined}
                  className={classNames('settings-mobile-top-nav__item', {
                    'settings-mobile-top-nav__item--active': active,
                  })}
                  onClick={() =>
                    navigateWithSettingsTransition(target, () => {
                      void navigate(target, { replace: true });
                    })
                  }
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
        <main className={contentClass} data-settings-page-content aria-busy={loading || undefined}>
          {loading ? <SettingsContentLoading /> : children}
        </main>
      </div>
    </SettingsViewModeProvider>
  );
};

export default SettingsPageWrapper;
