import React, { useEffect, useMemo, useRef, useState } from 'react';
import classNames from 'classnames';
import { ArrowCircleLeft, ArrowLeft, ArrowRight, ExpandLeft, ExpandRight } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import { ipcBridge } from '@/common';
import {
  isSameSessionTarget,
  type SessionTarget,
} from '@/common/types/ids';
import InstantHoverTooltip, { type InstantHoverTooltipProps } from '@renderer/components/base/InstantHoverTooltip';
import MobileConversationBrand from './MobileConversationBrand';
import TitlebarUpdateButton from './TitlebarUpdateButton';
import { useTitlebarContextTitle } from './useTitlebarContextTitle';
import WindowControls from '../WindowControls';
import { WORKSPACE_STATE_EVENT, dispatchWorkspaceToggleEvent } from '@renderer/utils/workspace/workspaceEvents';
import type { WorkspaceStateDetail } from '@renderer/utils/workspace/workspaceEvents';

import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useNavigationHistory } from '@/renderer/hooks/context/NavigationHistoryContext';
import { isDesktopShell, isMacOS } from '@/renderer/utils/platform';
import { parseSessionRoute } from '@/renderer/utils/routes/sessionRoute';
import './titlebar.css';

interface TitlebarProps {
  workspaceAvailable: boolean;
}

type TitlebarIconButtonOptions = {
  tooltip: string;
  className: string;
  children: React.ReactNode;
  disabled?: boolean;
  onClick?: () => void;
};

// Claude-desktop-style sidebar toggle icon: a rounded rectangle with a vertical divider
// near the left edge, indicating a collapsible side panel. Rendered as inline SVG since
// @icon-park doesn't ship this exact shape.
//
// Uses a 48-unit viewBox to match @icon-park's stroke scale, so passing the same
// `strokeWidth` value here and to @icon-park icons produces visually identical lines.
//
// The rect spans y=10..38 (height 28), slightly taller than @icon-park's
// ArrowLeft/ArrowRight (which span y=12..36) so the sidebar icon reads a
// touch larger. The rect remains centered at y=24, matching the arrows'
// centerline so all three icons stay on the same visual baseline.
const SidebarIcon: React.FC<{ size?: number; strokeWidth?: number }> = ({ size = 18, strokeWidth = 4 }) => (
  <svg
    width={size}
    height={size}
    viewBox='0 0 48 48'
    fill='none'
    stroke='currentColor'
    strokeWidth={strokeWidth}
    strokeLinecap='round'
    strokeLinejoin='round'
    aria-hidden='true'
    focusable='false'
  >
    <rect x='6' y='10' width='36' height='28' rx='5' />
    <line x1='18' y1='10' x2='18' y2='38' />
  </svg>
);

const NewConversationIcon: React.FC<{ size?: number; strokeWidth?: number }> = ({
  size = 18,
  strokeWidth = 4,
}) => (
  <svg
    width={size}
    height={size}
    viewBox='0 0 48 48'
    fill='none'
    stroke='currentColor'
    strokeWidth={strokeWidth}
    strokeLinecap='round'
    strokeLinejoin='round'
    aria-hidden='true'
    focusable='false'
  >
    <path d='M9 7h30a4 4 0 0 1 4 4v22a4 4 0 0 1-4 4H23l-10 7v-7H9a4 4 0 0 1-4-4V11a4 4 0 0 1 4-4Z' />
    <path d='M24 15v14M17 22h14' />
  </svg>
);

const HomeIcon: React.FC<{ size?: number; strokeWidth?: number }> = ({ size = 18, strokeWidth = 4 }) => (
  <svg
    width={size}
    height={size}
    viewBox='0 0 48 48'
    fill='none'
    stroke='currentColor'
    strokeWidth={strokeWidth}
    strokeLinecap='round'
    strokeLinejoin='round'
    aria-hidden='true'
    focusable='false'
  >
    <path d='m6 22 18-15 18 15v18a3 3 0 0 1-3 3H9a3 3 0 0 1-3-3V22Z' />
    <path d='M19 43V29h10v14' />
  </svg>
);

const Titlebar: React.FC<TitlebarProps> = ({ workspaceAvailable }) => {
  const { t } = useTranslation();
  const [workspaceCollapsed, setWorkspaceCollapsed] = useState(true);

  const [mobileCenterOffset, setMobileCenterOffset] = useState(0);
  const layout = useLayoutContext();
  const navigationHistory = useNavigationHistory();
  const location = useLocation();
  const navigate = useNavigate();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const lastNonSettingsPathRef = useRef('/guid');
  const activeWorkspaceTarget = useMemo<SessionTarget | null>(() => {
    return parseSessionRoute(location.pathname);
  }, [location.pathname]);
  const { title: contextTitle, activeConversationId, conversation } = useTitlebarContextTitle(location.pathname);

  // 监听工作空间折叠状态，保持按钮图标一致 / Sync workspace collapsed state for toggle button
  useEffect(() => {
    if (typeof window === 'undefined') {
      return undefined;
    }
    const handler = (event: Event) => {
      const customEvent = event as CustomEvent<WorkspaceStateDetail>;
      if (
        activeWorkspaceTarget &&
        customEvent.detail?.target &&
        isSameSessionTarget(customEvent.detail.target, activeWorkspaceTarget) &&
        typeof customEvent.detail.collapsed === 'boolean'
      ) {
        setWorkspaceCollapsed(customEvent.detail.collapsed);
      }
    };
    window.addEventListener(WORKSPACE_STATE_EVENT, handler as EventListener);
    return () => {
      window.removeEventListener(WORKSPACE_STATE_EVENT, handler as EventListener);
    };
  }, [activeWorkspaceTarget]);


  const isDesktopRuntime = isDesktopShell();
  const isMacRuntime = isDesktopRuntime && isMacOS();
  // Windows/Linux 显示自定义窗口按钮。
  const showWindowControls = isDesktopRuntime && !isMacRuntime;
  // Desktop workspace surfaces use the persistent far-right tool rail as their
  // single toggle. Mobile keeps the titlebar entry because the rail is hidden.
  const showWorkspaceButton = workspaceAvailable && Boolean(layout?.isMobile);

  const workspaceTooltip = workspaceCollapsed
    ? t('common.expandMore', { defaultValue: 'Expand workspace' })
    : t('common.collapse', { defaultValue: 'Collapse workspace' });
  const backToChatTooltip = t('common.back', { defaultValue: 'Back to Chat' });
  const isSettingsRoute = location.pathname.startsWith('/settings');
  const iconSize = 18;
  // Desktop uses slimmer strokes to match macOS-native chrome aesthetics;
  // mobile keeps the default weight so icons stay legible at larger sizes.
  const desktopIconStroke = layout?.isMobile ? undefined : 2.5;
  // 统一在标题栏左侧展示主侧栏开关 / Always expose sidebar toggle on titlebar left side
  const showSiderToggle = Boolean(layout?.setSiderCollapsed) && !(layout?.isMobile && isSettingsRoute);
  const showBackToChatButton = Boolean(layout?.isMobile && isSettingsRoute);
  const siderTooltip = layout?.siderCollapsed
    ? t('common.navExpand', { defaultValue: '展开应用导航' })
    : t('common.navCollapse', { defaultValue: '收起应用导航' });
  // 前进/后退仅在桌面端显示（移动端空间有限，保留原有的返回到聊天按钮）
  // Show back/forward on desktop only; mobile keeps the existing back-to-chat button.
  const showHistoryNav = Boolean(navigationHistory) && !layout?.isMobile;
  const historyBackTooltip = t('common.historyBack', { defaultValue: 'Back' });
  const historyForwardTooltip = t('common.forward', { defaultValue: 'Forward' });
  // The homepage already is the new-conversation surface. Keep this action for
  // a concrete chat, where it creates a useful escape hatch, and Settings,
  // where the Home icon returns to that surface.
  const showNewConversationAction =
    !layout?.isMobile && (isSettingsRoute || activeWorkspaceTarget?.kind === 'conversation');
  const newConversationTooltip = isSettingsRoute
    ? t('common.titlebar.home', { defaultValue: 'Home' })
    : t('terminal.newConversation');
  const handleSiderToggle = () => {
    if (!showSiderToggle || !layout?.setSiderCollapsed) return;
    layout.setSiderCollapsed(!layout.siderCollapsed);
  };

  const handleWorkspaceToggle = () => {
    if (!workspaceAvailable || !activeWorkspaceTarget) {
      return;
    }
    dispatchWorkspaceToggleEvent(activeWorkspaceTarget);
  };

  const handleBackToChat = () => {
    const target = lastNonSettingsPathRef.current;
    if (target && !target.startsWith('/settings')) {
      void navigate(target);
      return;
    }
    void navigate(-1);
  };

  // Windows/Linux: double-clicking the titlebar drag region toggles maximize,
  // matching native window behavior. Tauri's `data-tauri-drag-region` does NOT
  // implement this itself; we wire it on the frontend. Skipped on macOS (the OS
  // handles double-click on the native traffic-light chrome) and in the WebUI
  // browser (no window controls — `isDesktopRuntime` gates it). The event may
  // bubble from any descendant of the drag region, but never from an interactive
  // island explicitly marked as no-drag.
  const handleTitlebarDoubleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!isDesktopRuntime || isMacRuntime) return;
    const target = event.target as HTMLElement | null;
    if (!target?.closest('[data-tauri-drag-region]')) return;
    if (target.closest('[data-tauri-no-drag]')) return;
    void ipcBridge.windowControls.toggleMaximize.invoke();
  };

  useEffect(() => {
    if (!isSettingsRoute) {
      const path = `${location.pathname}${location.search}${location.hash}`;
      lastNonSettingsPathRef.current = path;
      try {
        sessionStorage.setItem('nomi:last-non-settings-path', path);
      } catch {
        // ignore
      }
      return;
    }
    try {
      const stored = sessionStorage.getItem('nomi:last-non-settings-path');
      if (stored) {
        lastNonSettingsPathRef.current = stored;
      }
    } catch {
      // ignore
    }
  }, [isSettingsRoute, location.pathname, location.search, location.hash]);

  useEffect(() => {
    if (!layout?.isMobile) {
      setMobileCenterOffset(0);
      return;
    }

    const updateOffset = () => {
      const leftWidth = menuRef.current?.offsetWidth || 0;
      const rightWidth = toolbarRef.current?.offsetWidth || 0;
      setMobileCenterOffset((leftWidth - rightWidth) / 2);
    };

    updateOffset();

    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', updateOffset);
      return () => window.removeEventListener('resize', updateOffset);
    }

    const observer = new ResizeObserver(() => updateOffset());
    if (containerRef.current) observer.observe(containerRef.current);
    if (menuRef.current) observer.observe(menuRef.current);
    if (toolbarRef.current) observer.observe(toolbarRef.current);

    return () => observer.disconnect();
  }, [layout?.isMobile, showBackToChatButton, showWorkspaceButton, contextTitle]);

  const mobileCenterStyle = layout?.isMobile
    ? ({
        '--app-titlebar-mobile-center-offset': `${workspaceAvailable ? mobileCenterOffset : 0}px`,
      } as React.CSSProperties)
    : undefined;

  const menuStyle: React.CSSProperties = useMemo(() => {
    if (!isMacRuntime || !showSiderToggle) return {};
    // macOS: sit the menu buttons right next to the traffic lights (which occupy ~70px).
    // Mobile keeps its own layout (no traffic lights).
    const marginLeft = layout?.isMobile ? '0px' : '76px';
    return {
      marginLeft,
    };
  }, [isMacRuntime, showSiderToggle, layout?.isMobile]);

  const renderIconButton = ({
    tooltip,
    className,
    children,
    disabled,
    onClick,
    position,
  }: TitlebarIconButtonOptions & { position?: InstantHoverTooltipProps['position'] }) => (
    <InstantHoverTooltip
      content={tooltip}
      position={position ?? 'bottom'}
      hoverDelayMs={400}
      className='app-titlebar__tooltip-anchor'
      dataTauriNoDrag
    >
      <button
        type='button'
        className={className}
        onClick={onClick}
        disabled={disabled}
        aria-label={tooltip}
        data-tauri-no-drag
      >
        {children}
      </button>
    </InstantHoverTooltip>
  );

  return (
    <div
      ref={containerRef}
      data-tauri-drag-region
      onDoubleClick={handleTitlebarDoubleClick}
      style={mobileCenterStyle}
      className={classNames('app-titlebar', {
        'app-titlebar--mobile': layout?.isMobile,
        'app-titlebar--mobile-conversation': layout?.isMobile && workspaceAvailable,
        'app-titlebar--wide': !layout?.isMobile,
        'app-titlebar--desktop': isDesktopRuntime,
        'app-titlebar--mac': isMacRuntime,
      })}
    >
      <div ref={menuRef} className='app-titlebar__menu' style={menuStyle}>
        <div className='app-titlebar__button-group' data-titlebar-group='navigation' data-tauri-no-drag>
          {showBackToChatButton &&
            renderIconButton({
              tooltip: backToChatTooltip,
              className: classNames('app-titlebar__button', layout?.isMobile && 'app-titlebar__button--mobile'),
              onClick: handleBackToChat,
              children: <ArrowCircleLeft theme='outline' size={iconSize} fill='currentColor' />,
            })}
          {showSiderToggle &&
            renderIconButton({
              tooltip: siderTooltip,
              className: classNames('app-titlebar__button', layout?.isMobile && 'app-titlebar__button--mobile'),
              onClick: handleSiderToggle,
              children: <SidebarIcon size={iconSize} strokeWidth={desktopIconStroke} />,
            })}
          {showHistoryNav && (
            <>
              {renderIconButton({
                tooltip: historyBackTooltip,
                className: 'app-titlebar__button app-titlebar__button--nav',
                onClick: () => navigationHistory?.back(),
                disabled: !navigationHistory?.canBack,
                children: (
                  <ArrowLeft theme='outline' size={iconSize} fill='currentColor' strokeWidth={desktopIconStroke} />
                ),
              })}
              {renderIconButton({
                tooltip: historyForwardTooltip,
                className: 'app-titlebar__button app-titlebar__button--nav',
                onClick: () => navigationHistory?.forward(),
                disabled: !navigationHistory?.canForward,
                children: (
                  <ArrowRight theme='outline' size={iconSize} fill='currentColor' strokeWidth={desktopIconStroke} />
                ),
              })}
            </>
          )}
        </div>
        {showNewConversationAction && (
          <div
            className='app-titlebar__button-group app-titlebar__button-group--context'
            data-titlebar-group='new-conversation'
            data-tauri-no-drag
          >
            <span className='app-titlebar__divider' aria-hidden='true' />
            {renderIconButton({
              tooltip: newConversationTooltip,
              className: 'app-titlebar__button app-titlebar__button--nav',
              onClick: () => navigate('/guid', { state: { resetPreset: true } }),
              children: isSettingsRoute ? (
                <HomeIcon size={iconSize} strokeWidth={desktopIconStroke} />
              ) : (
                <NewConversationIcon size={iconSize} strokeWidth={desktopIconStroke} />
              ),
            })}
          </div>
        )}
      </div>
      <div
        className={classNames('app-titlebar__brand', {
          'app-titlebar__brand--centered': layout?.isMobile,
        })}
        aria-label={layout?.isMobile ? contextTitle : undefined}
        data-tauri-drag-region
      >
        {layout?.isMobile ? (
          (() => {
            if (activeConversationId) {
              return <MobileConversationBrand conversation={conversation} fallbackTitle={contextTitle} />;
            }
            return (
              <span className='app-titlebar__brand-mobile'>
                <span className='app-titlebar__brand-text'>{contextTitle}</span>
              </span>
            );
          })()
        ) : null}
      </div>
      <div ref={toolbarRef} className='app-titlebar__toolbar'>
        {layout?.isMobile && (
          <div id='app-titlebar-actions-slot' className='app-titlebar__actions-slot' data-tauri-no-drag />
        )}
        <TitlebarUpdateButton
          iconSize={iconSize}
          strokeWidth={desktopIconStroke}
          className={classNames(layout?.isMobile && 'app-titlebar__button--mobile')}
        />
        {showWorkspaceButton && (
          renderIconButton({
            tooltip: workspaceTooltip,
            className: classNames('app-titlebar__button', layout?.isMobile && 'app-titlebar__button--mobile'),
            onClick: handleWorkspaceToggle,
            children: workspaceCollapsed ? (
              <ExpandRight theme='outline' size={iconSize} fill='currentColor' />
            ) : (
              <ExpandLeft theme='outline' size={iconSize} fill='currentColor' />
            ),
          })
        )}
        {showWindowControls && <WindowControls />}
      </div>
    </div>
  );
};

export default Titlebar;
