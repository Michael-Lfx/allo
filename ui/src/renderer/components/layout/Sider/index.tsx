import React, { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import classNames from 'classnames';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { Ghost, MessageOne, VideoOne } from '@icon-park/react';
import { cleanupSiderTooltips, getSiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { useAuth } from '@renderer/hooks/context/AuthContext';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useOptionalConversationHistoryContext } from '@renderer/hooks/context/ConversationHistoryContext';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import { blurActiveElement } from '@renderer/utils/ui/focus';
import { isDesktopShell } from '@renderer/utils/platform';
import { SERVER_MANAGED_MODELS } from '@/common/config/constants';
import { safeDecodeUriComponent } from '@/common/utils/localPath';
import WorkpathSessionList from '@renderer/pages/conversation/SessionList';
import CompanionSessionGroup from '@renderer/pages/conversation/SessionList/CompanionSessionGroup';
import { parseSessionRoute } from '@/renderer/utils/routes/sessionRoute';
import { useSidebarDisplayPreferences } from '@renderer/pages/conversation/SessionList/hooks/useSidebarDisplayPreferences';
import { useSlidingSelectionIndicator } from '@renderer/hooks/ui/useSlidingSelectionIndicator';
import { useSettingsNavigationTransition } from '@renderer/components/layout/SettingsNavigationTransition';
import { resolveSettingsTogglePath } from '@/renderer/utils/settingsToggle';
import {
  ConversationSiderActions,
  SiderConversationEntry,
  SiderNewConversationEntry,
  SiderSearchEntry,
  SiderKnowledgeEntry,
  SiderLearningEntry,
  SiderEvalEntry,
  SiderModelHubEntry,
  SiderRequirementsEntry,
  SiderScheduledEntry,
  SiderMeetingEntry,
  SiderSectionHeader,
  SiderVideoGenerationGroup,
} from './SiderNav';
import SiderFooter from './SiderFooter';
import { formatSiderAccountLabel } from './accountLabel';
import { historyTabAfterPathChange, type SiderHistoryTab } from './historyTab';
import styles from './Sider.module.css';
import SettingsSiderErrorBoundary from '../SettingsSiderErrorBoundary';
import { prefetchLearningPage } from '@renderer/pages/learning/prefetch';
import { prefetchNomiPage } from '@renderer/pages/nomi/prefetch';
import { prefetchVideoGenerationHome } from '@renderer/pages/videoGeneration/prefetch';

const SettingsSider = React.lazy(() => import('@renderer/pages/settings/components/SettingsSider'));

interface SiderProps {
  onSessionClick?: () => void;
  collapsed?: boolean;
}

/**
 * Sider — the app-level primary navigation rail.
 *
 * Slimmed down to a pure capability rail: the conversation/terminal session
 * list, the create switches, and full-text search were lifted out into the
 * content-area secondary sidebar (`ConversationShell` / `ContentSider`),
 * reached via the "会话" entry. The rail holds top-level destinations grouped
 * by small-text section headers (`SiderSectionHeader`): 工作 (会话 / 视频生成),
 * 资源 (知识库 / 学习 / 评测), 自动化 (定时任务), and a bottom-pinned
 * 设置 group (模型管理 + the footer). Execution engines live as an
 * independent tab inside Settings rather than being mixed into model
 * management.
 */
const Sider: React.FC<SiderProps> = ({ onSessionClick, collapsed = false }) => {
  const { t } = useTranslation();
  const { active: developerMode } = useDeveloperModeGate();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const location = useLocation();
  const { pathname, search, hash } = location;
  const navigate = useNavigate();
  const { navigateWithSettingsTransition } = useSettingsNavigationTransition();
  const { logout: localLogout, status: localStatus, user: localUser } = useAuth();
  const { logout: cloudLogout, status: cloudStatus, whoami } = useCloudAuth();
  const [batchMode, setBatchMode] = useState(false);
  const [workspaceActionsTarget, setWorkspaceActionsTarget] = useState<HTMLElement | null>(null);
  const siderRef = useRef<HTMLDivElement>(null);
  const { preferences: displayPreferences } = useSidebarDisplayPreferences();
  const isSettings = pathname.startsWith('/settings');
  const lastNonSettingsPathRef = useRef('/guid');
  const isDesktop = isDesktopShell();
  // WebUI: local admin session logout. Desktop: cloud account logout (local auth is always on).
  const showLocalLogout = !isDesktop && localStatus === 'authenticated';
  const showCloudLogout = isDesktop && cloudStatus === 'authenticated';
  const showLogout = showLocalLogout || showCloudLogout;
  const userLabel = useMemo(() => {
    if (showCloudLogout) {
      return formatSiderAccountLabel({
        nickname: whoami?.nickname,
        username: whoami?.username,
        email: whoami?.email,
      });
    }
    return formatSiderAccountLabel({
      username: localUser?.username ?? whoami?.username,
      email: whoami?.email,
    });
  }, [localUser?.username, showCloudLogout, whoami?.email, whoami?.nickname, whoami?.username]);
  const planLabel = whoami?.plan ?? '';

  const activeRoute = useMemo(() => parseSessionRoute(pathname), [pathname]);
  const activeConversationId = activeRoute?.kind === 'conversation' ? activeRoute.id : null;

  const [activeHistoryTab, setActiveHistoryTab] = useState<SiderHistoryTab>(() => {
    try {
      const saved = window.localStorage.getItem('flowy.sider.historyTab');
      if (saved === 'companions' || saved === 'video' || saved === 'workspaces') return saved;
    } catch {
      /* ignore */
    }
    return 'workspaces';
  });

  const handleSelectHistoryTab = useCallback((tab: SiderHistoryTab) => {
    setActiveHistoryTab(tab);
    try {
      window.localStorage.setItem('flowy.sider.historyTab', tab);
    } catch {
      /* ignore */
    }
  }, []);

  const isSessionRoute =
    pathname === '/guid' ||
    pathname.startsWith('/conversation/') ||
    pathname === '/terminal-new' ||
    pathname.startsWith('/terminal/');

  // Pathname-only: a tab click must not be overwritten just because the current
  // route still belongs to another domain (that was the snap-back flicker).
  useEffect(() => {
    setActiveHistoryTab((current) => {
      const next = historyTabAfterPathChange(pathname, current);
      if (next === current) return current;
      try {
        window.localStorage.setItem('flowy.sider.historyTab', next);
      } catch {
        /* ignore */
      }
      return next;
    });
  }, [pathname]);

  const selectionIndicator = useSlidingSelectionIndicator({
    containerRef: siderRef,
    activeSelector: '[data-sider-nav-entry][data-active="true"]:not([data-sider-selection-static])',
    revision: `${pathname}:${collapsed}`,
  });
  const { measureElement } = selectionIndicator;

  // Move the sliding indicator to the clicked entry on the urgent lane — before
  // the route subtree mounts — so the 240ms CSS transition starts on the
  // compositor thread and is immune to the heavy main-thread commit that follows.
  const handleSiderClick = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      const entry = (event.target as HTMLElement).closest<HTMLElement>('[data-sider-nav-entry]');
      if (!entry) return;
      // The settings footer toggle is selection-static: it swaps the whole
      // content region rather than sliding, so it must not move the indicator.
      if (entry.hasAttribute('data-sider-selection-static')) return;
      measureElement(entry);
    },
    [measureElement]
  );

  const conversationHistory = useOptionalConversationHistoryContext();
  const conversations = conversationHistory?.conversations;

  const lastActiveConversationPathRef = useRef<string | null>(null);

  useEffect(() => {
    if (pathname.startsWith('/conversation') || pathname.startsWith('/terminal')) {
      lastActiveConversationPathRef.current = `${pathname}${search}${hash}`;
    }
  }, [hash, pathname, search]);

  const getRecentConversationPath = useCallback(() => {
    // 1. If user visited a specific conversation/terminal in this session and it still exists
    if (lastActiveConversationPathRef.current) {
      const purePath = lastActiveConversationPathRef.current.split(/[?#]/)[0];
      const route = parseSessionRoute(purePath);
      if (route?.kind === 'conversation') {
        const exists = !conversations || conversations.some((c) => c.id === route.id);
        if (exists) {
          return lastActiveConversationPathRef.current;
        }
      } else {
        return lastActiveConversationPathRef.current;
      }
    }

    // 2. Otherwise find the latest conversation from history
    if (conversations && conversations.length > 0) {
      const sorted = [...conversations].sort(
        (a, b) => (b.modified_at || b.created_at || 0) - (a.modified_at || a.created_at || 0)
      );
      return `/conversation/${encodeURIComponent(sorted[0].id)}`;
    }

    // 3. Fallback to new chat page
    return '/guid';
  }, [conversations]);

  useEffect(() => {
    if (!pathname.startsWith('/settings')) {
      lastNonSettingsPathRef.current = `${pathname}${search}${hash}`;
    }
  }, [pathname, search, hash]);

  const navTo = useCallback(
    (target: string) => {
      cleanupSiderTooltips();
      blurActiveElement();
      Promise.resolve(navigate(target)).catch((error) => {
        console.error('Navigation failed:', error);
      });
      if (onSessionClick) {
        onSessionClick();
      }
    },
    [navigate, onSessionClick]
  );

  const handleConversationClick = useCallback(() => {
    navTo(getRecentConversationPath());
  }, [getRecentConversationPath, navTo]);

  const handleNewChat = useCallback(() => {
    cleanupSiderTooltips();
    blurActiveElement();
    Promise.resolve(navigate('/guid', { state: { resetPreset: true } })).catch((error) => {
      console.error('Navigation failed:', error);
    });
    if (onSessionClick) {
      onSessionClick();
    }
  }, [navigate, onSessionClick]);

  const handleConversationSelect = useCallback(() => {
    handleSelectHistoryTab('workspaces');
    if (onSessionClick) {
      onSessionClick();
    }
  }, [handleSelectHistoryTab, onSessionClick]);

  const compactDisplayPreferences = useMemo(() => {
    if (typeof localStorage !== 'undefined' && localStorage.getItem('nomifun:session-sidebar-display-preferences')) {
      return displayPreferences;
    }
    return {
      ...displayPreferences,
      preset: 'compact' as const,
      workpathNameMode: 'folder' as const,
      showGitBranch: false,
      sessionMetaMode: 'none' as const,
    };
  }, [displayPreferences]);

  const handleVideoGenerationHome = useCallback(() => {
    navTo('/video-generation');
  }, [navTo]);

  const isVideoRoute = pathname.startsWith('/video-generation');
  const isCompanionRoute = pathname.startsWith('/nomi');

  const activeBar2Module: SiderHistoryTab | null = useMemo(() => {
    if (isSessionRoute) return 'workspaces';
    if (isVideoRoute) return 'video';
    if (isCompanionRoute) return 'companions';
    return null;
  }, [isCompanionRoute, isSessionRoute, isVideoRoute]);

  const [optimisticBar2Module, setOptimisticBar2Module] = useState<SiderHistoryTab | null>(null);
  const lastPathnameRef = useRef(pathname);

  // Clear optimistic override once route catch-up happens
  useEffect(() => {
    if (optimisticBar2Module && activeBar2Module === optimisticBar2Module) {
      setOptimisticBar2Module(null);
    }
  }, [activeBar2Module, optimisticBar2Module]);

  // If the route changed to something else, clear optimistic override
  useEffect(() => {
    if (lastPathnameRef.current !== pathname) {
      lastPathnameRef.current = pathname;
      if (optimisticBar2Module && activeBar2Module !== optimisticBar2Module) {
        setOptimisticBar2Module(null);
      }
    }
  }, [pathname, optimisticBar2Module, activeBar2Module]);

  // Safety fallback: reset optimistic override if navigation takes unexpectedly long
  useEffect(() => {
    if (!optimisticBar2Module) return;
    const timer = window.setTimeout(() => {
      setOptimisticBar2Module(null);
    }, 3000);
    return () => window.clearTimeout(timer);
  }, [optimisticBar2Module]);

  const effectiveBar2Module: SiderHistoryTab | null = optimisticBar2Module ?? activeBar2Module;

  const handleTabClick = useCallback(
    (tab: SiderHistoryTab) => {
      setOptimisticBar2Module(tab);
      handleSelectHistoryTab(tab);
      if (tab === 'workspaces') {
        const recentPath = getRecentConversationPath();
        if (pathname !== recentPath) {
          navTo(recentPath);
        }
      } else if (tab === 'video') {
        if (!isVideoRoute) {
          handleVideoGenerationHome();
        }
      } else if (tab === 'companions') {
        if (!isCompanionRoute) {
          navTo('/nomi?tab=overview');
        }
      }
    },
    [
      getRecentConversationPath,
      handleSelectHistoryTab,
      handleVideoGenerationHome,
      isCompanionRoute,
      isVideoRoute,
      navTo,
      pathname,
    ]
  );

  const activeVideoGenerationSessionId = useMemo(() => {
    const m = pathname.match(/^\/video-generation\/([^/]+)\/?$/);
    const id = m?.[1] ? safeDecodeUriComponent(m[1]) : null;
    if (!id || id === 'campaigns' || id === 'clip' || id === 'canvas' || id === 'briefing') {
      return null;
    }
    return id;
  }, [pathname]);

  // Match clip task route: /video-generation/clip/:taskId — must NOT match the
  // bare /video-generation or any videoGeneration workspace session routes.
  const activeClipTaskId = useMemo(() => {
    const m = pathname.match(/^\/video-generation\/clip\/([^/]+)\/?$/);
    return m?.[1] ? safeDecodeUriComponent(m[1]) : null;
  }, [pathname]);

  const activeCanvasProjectId = useMemo(() => {
    const m = pathname.match(/^\/video-generation\/canvas\/([^/]+)\/?$/);
    return m?.[1] ? safeDecodeUriComponent(m[1]) : null;
  }, [pathname]);

  const activeBriefingId = useMemo(() => {
    const m = pathname.match(/^\/video-generation\/briefing\/([^/]+)\/?$/);
    return m?.[1] ? safeDecodeUriComponent(m[1]) : null;
  }, [pathname]);

  const handleOpenRecentVideoGeneration = useCallback(
    (sessionId: string) => {
      navTo(`/video-generation/${encodeURIComponent(sessionId)}`);
    },
    [navTo]
  );

  const handleOpenRecentClipTask = useCallback(
    (taskId: string) => {
      navTo(`/video-generation/clip/${encodeURIComponent(taskId)}`);
    },
    [navTo]
  );

  const handleOpenRecentCanvasProject = useCallback(
    (projectId: string) => {
      navTo(`/video-generation/canvas/${encodeURIComponent(projectId)}`);
    },
    [navTo]
  );

  const handleOpenRecentBriefing = useCallback(
    (briefingId: string) => {
      navTo(`/video-generation/briefing/${encodeURIComponent(briefingId)}`);
    },
    [navTo]
  );
  const handleScheduledClick = () => {
    setOptimisticBar2Module(null);
    navTo('/scheduled');
  };
  const handleMeetingClick = () => {
    setOptimisticBar2Module(null);
    navTo('/meeting');
  };
  const handleKnowledgeClick = () => {
    setOptimisticBar2Module(null);
    navTo('/knowledge');
  };
  const handleNomiClick = () => {
    setOptimisticBar2Module('companions');
    handleSelectHistoryTab('companions');
    navTo('/nomi');
  };
  const handleLearningClick = () => {
    setOptimisticBar2Module(null);
    navTo('/learn');
  };

  useEffect(() => {
    if (isSettings) return;
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    const warmIdleRoutes = () => {
      prefetchLearningPage();
      prefetchNomiPage();
      prefetchVideoGenerationHome();
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(warmIdleRoutes, {
        timeout: 1800,
      });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(warmIdleRoutes, 250);
    return () => window.clearTimeout(timer);
  }, [isSettings]);

  const handleEvalClick = () => {
    setOptimisticBar2Module(null);
    navTo('/eval');
  };
  const handleRequirementsClick = () => navTo('/requirements');
  const handlePresetClick = () => navTo('/presets');
  const handleSkillsClick = () => navTo('/skills');
  const handleMcpClick = () => navTo('/mcp');
  
  const handleSettingsClick = () => {
    setOptimisticBar2Module(null);
    cleanupSiderTooltips();
    blurActiveElement();
    const target = resolveSettingsTogglePath(pathname, lastNonSettingsPathRef.current);
    const go = () => {
      Promise.resolve(navigate(target.path)).catch((error) => {
        console.error('Navigation failed:', error);
      });
    };
    if (target.enter) {
      navigateWithSettingsTransition(target.path, go);
    } else {
      go();
    }
    if (onSessionClick) {
      onSessionClick();
    }
  };

  const handleLogout = useCallback(async () => {
    cleanupSiderTooltips();
    blurActiveElement();
    try {
      if (showCloudLogout) {
        await cloudLogout();
      } else {
        await localLogout();
      }
    } catch (error) {
      console.error('Logout failed:', error);
      return; // logout 失败时不执行后续操作
    }
    if (onSessionClick) {
      onSessionClick();
    }
  }, [cloudLogout, localLogout, onSessionClick, showCloudLogout]);

  useEffect(() => {
    if (!showLogout) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'l') {
        event.preventDefault();
        handleLogout();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [handleLogout, showLogout]);

  const tooltipEnabled = collapsed && !isMobile;
  const siderTooltipProps = getSiderTooltipProps(tooltipEnabled);

  return (
    <div
      id='flowy-primary-sider'
      ref={siderRef}
      className={`${styles.sider} size-full flex flex-col`}
      onClick={handleSiderClick}
    >
      <span
        aria-hidden='true'
        className={styles.selectionIndicator}
        data-visible={selectionIndicator.visible ? 'true' : 'false'}
        style={{
          width: selectionIndicator.width,
          height: selectionIndicator.height,
          transform: `translate(${selectionIndicator.left}px, ${selectionIndicator.top}px)`,
        }}
      />
      {/* Main content area */}
      {isSettings ? (
        <div className='flex-1 min-h-0 overflow-y-auto overflow-x-hidden'>
          <SettingsSiderErrorBoundary resetKey={`${pathname}${search}${hash}`}>
            <Suspense fallback={<div className='size-full' />}>
              <SettingsSider collapsed={collapsed} tooltipEnabled={tooltipEnabled} />
            </Suspense>
          </SettingsSiderErrorBoundary>
        </div>
      ) : (
        <div className='flex-1 min-h-0 flex flex-col'>
          <div
            data-testid='sider-primary-nav'
            className={`${styles.primaryNav} shrink-0 flex flex-col gap-2px`}
          >
            {/* 顶层高频操作：新建会话与搜索 */}
            <div className='flex flex-col gap-4px mb-4px'>
              <SiderNewConversationEntry
                isMobile={isMobile}
                collapsed={collapsed}
                siderTooltipProps={siderTooltipProps}
                onClick={handleNewChat}
              />
              <SiderSearchEntry
                isMobile={isMobile}
                collapsed={collapsed}
                siderTooltipProps={siderTooltipProps}
                onConversationSelect={handleConversationSelect}
                onSessionClick={onSessionClick}
              />
            </div>

            {/* 业务功能横向 Dock 栏（展开态下高度仅 34px，容纳 5 个无历史纯功能模块；收起态下恢复纵向一列） */}
            <div
              role={collapsed ? undefined : 'toolbar'}
              aria-label={t('common.titlebar.sections.work', { defaultValue: '功能导航' })}
              className={classNames(
                collapsed
                  ? 'flex flex-col gap-2px'
                  : 'flex items-center gap-2px p-2px my-2px rd-8px bg-fill-1 border border-solid border-[var(--color-border-2)]'
              )}
            >
              <SiderSectionHeader label={t('common.titlebar.sections.work')} collapsed={collapsed} compact hidden />
              {collapsed && (
                <>
                  <SiderConversationEntry
                    isMobile={isMobile}
                    isActive={isSessionRoute}
                    collapsed={collapsed}
                    siderTooltipProps={siderTooltipProps}
                    onClick={handleConversationClick}
                  />
                  <SiderVideoGenerationGroup
                    isMobile={isMobile}
                    moduleActive={pathname.startsWith('/video-generation')}
                    activeSessionId={activeVideoGenerationSessionId}
                    activeClipTaskId={activeClipTaskId}
                    activeCanvasProjectId={activeCanvasProjectId}
                    activeBriefingId={activeBriefingId}
                    collapsed={collapsed}
                    siderTooltipProps={siderTooltipProps}
                    onEnterHome={handleVideoGenerationHome}
                    onOpenProject={handleOpenRecentVideoGeneration}
                    onOpenClipTask={handleOpenRecentClipTask}
                    onOpenCanvasProject={handleOpenRecentCanvasProject}
                    onOpenBriefing={handleOpenRecentBriefing}
                  />
                </>
              )}

              <SiderSectionHeader
                label={t('common.titlebar.sections.resources')}
                collapsed={collapsed}
                compact
                hidden
              />
              <SiderKnowledgeEntry
                isMobile={isMobile}
                isActive={!effectiveBar2Module && pathname.startsWith('/knowledge')}
                collapsed={collapsed}
                dock={!collapsed}
                siderTooltipProps={siderTooltipProps}
                onClick={handleKnowledgeClick}
              />
              <SiderLearningEntry
                isMobile={isMobile}
                isActive={!effectiveBar2Module && pathname.startsWith('/learn')}
                collapsed={collapsed}
                dock={!collapsed}
                siderTooltipProps={siderTooltipProps}
                onClick={handleLearningClick}
              />

              <SiderSectionHeader
                label={t('common.titlebar.sections.automation')}
                collapsed={collapsed}
                compact
                hidden
              />
              <SiderScheduledEntry
                isMobile={isMobile}
                isActive={!effectiveBar2Module && pathname === '/scheduled'}
                collapsed={collapsed}
                dock={!collapsed}
                siderTooltipProps={siderTooltipProps}
                onClick={handleScheduledClick}
              />
              <SiderMeetingEntry
                isMobile={isMobile}
                isActive={!effectiveBar2Module && pathname.startsWith('/meeting')}
                collapsed={collapsed}
                dock={!collapsed}
                siderTooltipProps={siderTooltipProps}
                onClick={handleMeetingClick}
              />
              {developerMode === true && (
                <SiderEvalEntry
                  isMobile={isMobile}
                  isActive={!effectiveBar2Module && pathname.startsWith('/eval')}
                  collapsed={collapsed}
                  dock={!collapsed}
                  siderTooltipProps={siderTooltipProps}
                  onClick={handleEvalClick}
                />
              )}
            </div>
          </div>
          {/* 项目/工作路径树 — 独立滚动，一级菜单保持固定 */}
          {!collapsed && (
            <section
              data-testid='sider-workspaces-section'
              aria-labelledby='flowy-workspaces-heading'
              className={styles.workspaceSection}
            >
              <SiderSectionHeader
                id='flowy-workspaces-heading'
                label={t('common.titlebar.sections.workspaces')}
                collapsed={false}
                compact
                hidden
              />

              {/* 三段式会话分类切换器：区分项目、视频创作、桌宠 */}
              <div className='w-full my-2px shrink-0'>
                <div
                  role='tablist'
                  aria-label={t('common.titlebar.sections.workspaces', { defaultValue: '工作区' })}
                  className='relative flex items-center p-2px rd-8px bg-fill-1 border border-solid border-[var(--color-border-2)]'
                >
                  <span
                    aria-hidden='true'
                    className='absolute h-26px rd-6px bg-fill-3 text-t-primary shadow-sm transition-transform duration-220 ease-[cubic-bezier(0.25,1,0.5,1)] pointer-events-none'
                    style={{
                      width: 'calc((100% - 4px) / 3)',
                      left: '2px',
                      top: '2px',
                      transform: `translateX(${
                        effectiveBar2Module === 'video' ? '100%' : effectiveBar2Module === 'companions' ? '200%' : '0%'
                      })`,
                      opacity: effectiveBar2Module ? 1 : 0,
                    }}
                  />
                  <button
                    type='button'
                    role='tab'
                    title={t('sessionList.projectsTab', { defaultValue: '项目' })}
                    aria-selected={effectiveBar2Module === 'workspaces'}
                    onClick={() => handleTabClick('workspaces')}
                    className={classNames(
                      'relative z-1 group flex-1 h-26px px-4px text-12px font-[500] rd-6px flex items-center justify-center gap-4px transition-colors duration-180 cursor-pointer border-none select-none whitespace-nowrap overflow-hidden text-ellipsis bg-transparent',
                      effectiveBar2Module === 'workspaces'
                        ? 'text-t-primary'
                        : 'text-t-tertiary hover:text-t-primary'
                    )}
                  >
                    <MessageOne
                      theme='outline'
                      size={15}
                      fill='currentColor'
                      className={classNames(
                        'block leading-none shrink-0 transition-colors duration-180',
                        effectiveBar2Module === 'workspaces'
                          ? 'text-primary-6'
                          : 'text-t-tertiary group-hover:text-t-primary'
                      )}
                      style={{ lineHeight: 0 }}
                    />
                    <span className='truncate'>{t('sessionList.projectsTab', { defaultValue: '项目' })}</span>
                  </button>
                  <button
                    type='button'
                    role='tab'
                    title={t('videoGeneration.nav.shortTitle', { defaultValue: '视频' })}
                    aria-selected={effectiveBar2Module === 'video'}
                    onClick={() => handleTabClick('video')}
                    onPointerEnter={() => prefetchVideoGenerationHome()}
                    className={classNames(
                      'relative z-1 group flex-1 h-26px px-4px text-12px font-[500] rd-6px flex items-center justify-center gap-4px transition-colors duration-180 cursor-pointer border-none select-none whitespace-nowrap overflow-hidden text-ellipsis bg-transparent',
                      effectiveBar2Module === 'video'
                        ? 'text-t-primary'
                        : 'text-t-tertiary hover:text-t-primary'
                    )}
                  >
                    <VideoOne
                      theme='outline'
                      size={15}
                      fill='currentColor'
                      className={classNames(
                        'block leading-none shrink-0 transition-colors duration-180',
                        effectiveBar2Module === 'video'
                          ? 'text-primary-6'
                          : 'text-t-tertiary group-hover:text-t-primary'
                      )}
                      style={{ lineHeight: 0 }}
                    />
                    <span className='truncate'>{t('videoGeneration.nav.shortTitle', { defaultValue: '视频' })}</span>
                  </button>
                  <button
                    type='button'
                    role='tab'
                    title={t('nomi.shortTitle', { defaultValue: '桌宠' })}
                    aria-selected={effectiveBar2Module === 'companions'}
                    onClick={() => handleTabClick('companions')}
                    onPointerEnter={() => prefetchNomiPage()}
                    className={classNames(
                      'relative z-1 group flex-1 h-26px px-4px text-12px font-[500] rd-6px flex items-center justify-center gap-4px transition-colors duration-180 cursor-pointer border-none select-none whitespace-nowrap overflow-hidden text-ellipsis bg-transparent',
                      effectiveBar2Module === 'companions'
                        ? 'text-t-primary'
                        : 'text-t-tertiary hover:text-t-primary'
                    )}
                  >
                    <Ghost
                      theme='outline'
                      size={15}
                      fill='currentColor'
                      className={classNames(
                        'block leading-none shrink-0 transition-colors duration-180',
                        effectiveBar2Module === 'companions'
                          ? 'text-primary-6'
                          : 'text-t-tertiary group-hover:text-t-primary'
                      )}
                      style={{ lineHeight: 0 }}
                    />
                    <span className='truncate'>{t('nomi.shortTitle', { defaultValue: '桌宠' })}</span>
                  </button>
                </div>
              </div>

              {/* 当处于项目 Tab 时，展示专门的操作工具条（左侧显示「项目/工作路径」，右侧折叠全部 / 添加工作区），在其他两个 tab 下完全隐藏 */}
              <div
                style={{ display: activeHistoryTab === 'workspaces' ? 'flex' : 'none' }}
                className='items-center justify-between px-8px pt-3px pb-2px text-11px text-t-tertiary select-none shrink-0'
              >
                <span className='font-[500]'>{t('sessionList.workpathSection', { defaultValue: '工作区' })}</span>
                <span
                  ref={setWorkspaceActionsTarget}
                  data-testid='sider-workspace-actions-target'
                  className='flex items-center gap-2px'
                />
              </div>

              <div
                data-testid='sider-workspaces-scroll-area'
                className={`${styles.scrollArea} flex-1 min-h-0 overflow-y-auto overflow-x-hidden pt-0 pb-8px`}
              >
                <div hidden={activeHistoryTab !== 'workspaces'}>
                  <WorkpathSessionList
                    collapsed={false}
                    tooltipEnabled={false}
                    batchMode={batchMode}
                    displayPreferences={compactDisplayPreferences}
                    onBatchModeChange={setBatchMode}
                    workspaceActionsTarget={workspaceActionsTarget}
                    embeddedInPrimarySider
                    hideCompanionGroup={true}
                  />
                </div>
                <div hidden={activeHistoryTab !== 'companions'} className='px-4px py-2px'>
                  <CompanionSessionGroup
                    activeConversationId={activeConversationId}
                    onSessionClick={onSessionClick}
                    expanded={true}
                    hideHeader={true}
                  />
                </div>
                <div hidden={activeHistoryTab !== 'video'} className='px-4px py-2px'>
                  <SiderVideoGenerationGroup
                    isMobile={isMobile}
                    moduleActive={pathname.startsWith('/video-generation')}
                    activeSessionId={activeVideoGenerationSessionId}
                    activeClipTaskId={activeClipTaskId}
                    activeCanvasProjectId={activeCanvasProjectId}
                    activeBriefingId={activeBriefingId}
                    collapsed={false}
                    dock={false}
                    flat={true}
                    siderTooltipProps={siderTooltipProps}
                    onEnterHome={handleVideoGenerationHome}
                    onOpenProject={handleOpenRecentVideoGeneration}
                    onOpenClipTask={handleOpenRecentClipTask}
                    onOpenCanvasProject={handleOpenRecentCanvasProject}
                    onOpenBriefing={handleOpenRecentBriefing}
                  />
                </div>
              </div>
            </section>
          )}
        </div>
      )}
      {/* Bottom pinned group (设置) — Model & Agent sit directly above Settings */}
      <div className='shrink-0 mt-auto pt-8px flex flex-col gap-2px border-t border-solid border-[var(--color-border-2)] border-l-0 border-r-0 border-b-0'>
        {!SERVER_MANAGED_MODELS && (
          <SiderModelHubEntry
            isMobile={isMobile}
            isActive={pathname.startsWith('/models')}
            collapsed={collapsed}
            siderTooltipProps={siderTooltipProps}
            onClick={() => navTo('/models')}
          />
        )}
        <SiderFooter
          isMobile={isMobile}
          isSettings={isSettings}
          collapsed={collapsed}
          siderTooltipProps={siderTooltipProps}
          userLabel={userLabel}
          planLabel={planLabel}
          showLogout={showLogout}
          showEditNickname={showCloudLogout}
          onLogout={handleLogout}
          onOpenCompanion={handleNomiClick}
          onSettingsClick={handleSettingsClick}
        />
      </div>
    </div>
  );
};

export default Sider;
