import React, { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { cleanupSiderTooltips, getSiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { useAuth } from '@renderer/hooks/context/AuthContext';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { blurActiveElement } from '@renderer/utils/ui/focus';
import { isDesktopShell } from '@renderer/utils/platform';
import { SERVER_MANAGED_MODELS } from '@/common/config/constants';
import WorkpathSessionList from '@renderer/pages/conversation/SessionList';
import { useSidebarDisplayPreferences } from '@renderer/pages/conversation/SessionList/hooks/useSidebarDisplayPreferences';
import {
  ConversationSiderActions,
  SiderConversationEntry,
  SiderKnowledgeEntry,
  SiderLearningEntry,
  SiderModelHubEntry,
  SiderNomiEntry,
  SiderRequirementsEntry,
  SiderScheduledEntry,
  SiderSectionHeader,
  SiderVideoGenerationGroup,
} from './SiderNav';
import SiderFooter from './SiderFooter';
import styles from './Sider.module.css';
import SettingsSiderErrorBoundary from '../SettingsSiderErrorBoundary';

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
 * by small-text section headers (`SiderSectionHeader`): 常用 (会话 / 桌面伙伴),
 * 对外服务 (对外伙伴), 数据空间 (学习 / 知识库), 自动化 (定时任务 / 需求平台),
 * 增强工具 (设定 / Skill / MCP), and a bottom-pinned 设置 group
 * (模型管理 + the footer). Execution engines live as an
 * independent tab inside Settings rather than being mixed into model
 * management.
 */
const Sider: React.FC<SiderProps> = ({ onSessionClick, collapsed = false }) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const location = useLocation();
  const { pathname, search, hash } = location;
  const navigate = useNavigate();
  const { logout: localLogout, status: localStatus, user: localUser } = useAuth();
  const { logout: cloudLogout, status: cloudStatus, whoami } = useCloudAuth();
  const [batchMode, setBatchMode] = useState(false);
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
      return whoami?.email ?? whoami?.username ?? '';
    }
    return localUser?.username ?? whoami?.email ?? whoami?.username ?? '';
  }, [localUser?.username, showCloudLogout, whoami?.email, whoami?.username]);
  const planLabel = whoami?.plan ?? '';

  const isSessionRoute =
    pathname === '/guid' ||
    pathname.startsWith('/conversation/') ||
    pathname === '/terminal-new' ||
    pathname.startsWith('/terminal/');

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

  const handleConversationClick = () => navTo('/guid');
  const handleVideoGenerationHome = () => {
    navTo('/video-generation');
  };

  const activeVideoGenerationSessionId = useMemo(() => {
    const m = pathname.match(/^\/video-generation\/([^/]+)\/?$/);
    return m?.[1] ? decodeURIComponent(m[1]) : null;
  }, [pathname]);

  const handleOpenRecentVideoGeneration = useCallback(
    (sessionId: string) => {
      navTo(`/video-generation/${encodeURIComponent(sessionId)}`);
    },
    [navTo]
  );
  const handleScheduledClick = () => navTo('/scheduled');
  const handleKnowledgeClick = () => navTo('/knowledge');
  const handleNomiClick = () => navTo('/nomi');
  const handleLearningClick = () => navTo('/learn');
  const handleRequirementsClick = () => navTo('/requirements');
  const handlePresetClick = () => navTo('/presets');
  const handleSkillsClick = () => navTo('/skills');
  const handleMcpClick = () => navTo('/mcp');
  
  const handleSettingsClick = () => {
    cleanupSiderTooltips();
    blurActiveElement();
    if (isSettings) {
      const target = lastNonSettingsPathRef.current || '/guid';
      Promise.resolve(navigate(target)).catch((error) => {
        console.error('Navigation failed:', error);
      });
    } else {
      Promise.resolve(navigate('/settings/system')).catch((error) => {
        console.error('Navigation failed:', error);
      });
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
    <div className={`${styles.sider} size-full flex flex-col`}>
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
          <div data-testid='sider-primary-nav' className='shrink-0 flex flex-col gap-2px'>
            {/* 会话 — 一级菜单入口 */}
            <SiderConversationEntry
              isMobile={isMobile}
              isActive={isSessionRoute}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleConversationClick}
            />
            {/* ViMax video generation — collapsible recent projects (workpath-aligned) */}
            <SiderVideoGenerationGroup
              isMobile={isMobile}
              moduleActive={pathname.startsWith('/video-generation')}
              activeSessionId={activeVideoGenerationSessionId}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onEnterHome={handleVideoGenerationHome}
              onOpenProject={handleOpenRecentVideoGeneration}
            />
            <SiderKnowledgeEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/knowledge')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleKnowledgeClick}
            />
            <SiderLearningEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/learn')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleLearningClick}
            />
            {/* 自动化 — automation platforms */}
            <SiderScheduledEntry
              isMobile={isMobile}
              isActive={pathname === '/scheduled'}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleScheduledClick}
            />
          </div>
          {/* 项目/工作路径树 — 独立滚动，一级菜单保持固定 */}
          {!collapsed && (
            <div
              data-testid='sider-workspaces-scroll-area'
              className={`${styles.scrollArea} flex-1 min-h-0 overflow-y-auto overflow-x-hidden pl-5px pr-8px pt-2px pb-8px`}
            >
              <WorkpathSessionList
                collapsed={false}
                tooltipEnabled={false}
                batchMode={batchMode}
                displayPreferences={displayPreferences}
                onBatchModeChange={setBatchMode}
              />
            </div>
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
          onLogout={handleLogout}
          onSettingsClick={handleSettingsClick}
        />
      </div>
    </div>
  );
};

export default Sider;
