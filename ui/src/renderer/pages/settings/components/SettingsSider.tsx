import FlexFullContainer from '@/renderer/components/layout/FlexFullContainer';
import { useSlidingSelectionIndicator } from '@/renderer/hooks/ui/useSlidingSelectionIndicator';
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
  Server,
  System,
  Tool,
  TreeDiagram,
} from '@icon-park/react';
import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { Tooltip } from '@arco-design/web-react';
import { getSiderTooltipProps } from '@/renderer/utils/ui/siderTooltip';
import {
  isSettingsNavItemActive,
  type SettingsNavIcon,
  type SettingsNavItem,
  useSettingsNavigation,
} from './settingsNavigation';
import { prefetchSettingsPages } from '../prefetch';
import './settings.css';

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

const SettingsNavIconSlot: React.FC<{ item: SettingsNavItem; selected: boolean }> = ({ item, selected }) => {
  if (item.icon === 'extension' && item.iconUrl) {
    return <img src={item.iconUrl} alt='' className='size-16px object-contain' />;
  }

  const Icon = item.icon === 'extension' ? Puzzle : iconByName[item.icon];
  return (
    <Icon
      theme='outline'
      size='16'
      strokeWidth={3}
      className={selected ? 'block leading-none text-primary-6' : 'block leading-none text-t-secondary'}
    />
  );
};

const SettingsSider: React.FC<{ collapsed?: boolean; tooltipEnabled?: boolean }> = ({
  collapsed = false,
  tooltipEnabled = false,
}) => {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { groups } = useSettingsNavigation();
  const navigationRef = useRef<HTMLDivElement>(null);
  const menuSignature = useMemo(
    () => groups.flatMap((group) => group.items.map((item) => item.id)).join('|'),
    [groups]
  );
  const selectionIndicator = useSlidingSelectionIndicator({
    containerRef: navigationRef,
    activeSelector: '[data-settings-nav-entry][data-active="true"]',
    revision: `${pathname}:${collapsed}:${menuSignature}`,
  });
  const { measureElement } = selectionIndicator;

  // Move the settings indicator to the clicked entry on the urgent lane — before
  // the settings content mounts — so the slide animation starts immediately and
  // is not blocked by the heavy main-thread commit of the target panel.
  const handleSettingsNavClick = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      const entry = (event.target as HTMLElement).closest<HTMLElement>('[data-settings-nav-entry]');
      if (!entry) return;
      measureElement(entry);
    },
    [measureElement]
  );

  // Footer idle prefetch is skipped while pathname is under /settings; warm the
  // sibling panels again on mount so intelligence-group clicks are not cold.
  useEffect(() => {
    prefetchSettingsPages();
  }, []);

  useEffect(() => {
    navigationRef.current
      ?.querySelector<HTMLElement>('[data-settings-nav-entry][data-active="true"]')
      ?.scrollIntoView({ block: 'nearest' });
  }, [menuSignature, pathname]);

  const siderTooltipProps = getSiderTooltipProps(tooltipEnabled);
  return (
    <div
      ref={navigationRef}
      className={classNames('settings-sider relative h-full flex flex-col gap-2px overflow-y-auto overflow-x-hidden', {
        'settings-sider--collapsed': collapsed,
      })}
      onClick={handleSettingsNavClick}
    >
      <span
        aria-hidden='true'
        className='settings-sider__selection-indicator'
        data-visible={selectionIndicator.visible ? 'true' : 'false'}
        style={{
          transform: `translate3d(${selectionIndicator.left}px, ${selectionIndicator.top}px, 0)`,
          width: selectionIndicator.width,
          height: selectionIndicator.height,
        }}
      />
      {groups.map((group) => (
        <section key={group.id} className='settings-sider__group relative z-1' aria-label={group.label}>
          {!collapsed && (
            <h2 className='settings-sider__group-header mb-0 mt-8px h-28px flex items-center px-12px text-14px font-500 text-t-tertiary select-none'>
              {group.label}
            </h2>
          )}
          {group.items.map((item) => {
            const selected = isSettingsNavItemActive(pathname, item);
            const target = item.path.startsWith('/') ? item.path : `/settings/${item.path}`;
            return (
              <Tooltip key={item.id} {...siderTooltipProps} content={item.label} position='right'>
                <button
                  type='button'
                  data-settings-nav-entry='true'
                  data-settings-id={item.id}
                  data-settings-path={item.path}
                  data-active={selected ? 'true' : 'false'}
                  aria-current={selected ? 'page' : undefined}
                  className={classNames(
                    'settings-sider__item h-34px rd-8px relative z-1 flex shrink-0 items-center gap-8px overflow-hidden text-left transition-colors',
                    collapsed ? 'w-full justify-center px-0' : 'w-full justify-start px-10px',
                    { 'hover:bg-fill-2': !selected }
                  )}
                  onClick={() => {
                    Promise.resolve(navigate(target, { replace: true })).catch((error: unknown) => {
                      console.error('Navigation failed:', error);
                    });
                  }}
                >
                  <span className='size-22px flex shrink-0 items-center justify-center line-height-0'>
                    <SettingsNavIconSlot item={item} selected={selected} />
                  </span>
                  <FlexFullContainer className='h-24px collapsed-hidden'>
                    <span
                      className={classNames(
                        'settings-sider__item-label inline-block w-full overflow-hidden text-nowrap text-14px font-500 lh-24px whitespace-nowrap',
                        selected ? 'text-primary-6' : 'text-t-primary'
                      )}
                    >
                      {item.label}
                    </span>
                  </FlexFullContainer>
                </button>
              </Tooltip>
            );
          })}
        </section>
      ))}
    </div>
  );
};

export default SettingsSider;
