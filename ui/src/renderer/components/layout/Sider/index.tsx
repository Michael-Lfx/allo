

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
import { useKnowledgeInboxPending } from '@renderer/pages/knowledge/useKnowledge';
import {
  SiderConfigGroup,
  SiderConversationEntry,
  SiderKnowledgeEntry,
  SiderModelHubEntry,
  SiderNomiEntry,
  SiderOpenCapabilitiesEntry,
  SiderPublicServiceEntry,
  SiderRequirementsEntry,
  SiderScheduledEntry,
  SiderSectionHeader,
  SiderVideoGenerationEntry,
} from './SiderNav';
import SiderFooter from './SiderFooter';
import { useFirstWinMode } from '@/renderer/utils/onboarding/firstWinMode';

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
 * 对外服务 (对外伙伴), 数据空间 (知识库), 自动化 (定时任务 / 需求平台),
 * 增强工具 (设定 / Skill / MCP), and a bottom-pinned 设置 group
 * (模型管理 + the footer). Execution engines live as an independent tab
 * inside Settings rather than being mixed into model management.
 */
const Sider: React.FC<SiderProps> = ({ onSessionClick, collapsed = false }) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const location = useLocation();
  const { pathname, search, hash } = location;
  const { count: pendingInboxCount } = useKnowledgeInboxPending();
  const { isFirstWin } = useFirstWinMode();

  const navigate = useNavigate();
  const { logout: localLogout, status: localStatus, user: localUser } = useAuth();
  const { logout: cloudLogout, status: cloudStatus, whoami } = useCloudAuth();
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
  const [capabilitiesExpanded, setCapabilitiesExpanded] = useState(() => {
    if (typeof window === 'undefined') return false;
    try {
      return window.localStorage.getItem('flowy.sider.capabilitiesExpanded') === 'true';
    } catch {
      return false;
    }
  });

  // The "会话" entry stays active across every route owned by ConversationShell.
  const isSessionRoute =
    pathname === '/guid' ||
    pathname.startsWith('/conversation/') ||
    pathname === '/terminal-new' ||
    pathname.startsWith('/terminal/');

  // First-win focus: keep the rail on New Task + essentials until the user
  // confirms a reviewable result. Returning users always see the full rail.
  const showCapabilityHub = !isFirstWin || capabilitiesExpanded || !isSessionRoute;

  const toggleCapabilities = useCallback(() => {
    setCapabilitiesExpanded((current) => {
      const next = !current;
      try {
        window.localStorage.setItem('flowy.sider.capabilitiesExpanded', String(next));
      } catch {
        return next;
      }
      return next;
    });
  }, []);

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
  const handleScheduledClick = () => navTo('/scheduled');
  const handleRequirementsClick = () => navTo('/requirements');
  const handleKnowledgeClick = () => navTo('/knowledge');
  const handleNomiClick = () => navTo('/nomi');
  const handleVideoGenerationClick = () => navTo('/video-generation');
  const handlePublicServiceClick = () => navTo('/public-companions');
  const handleOpenCapabilitiesClick = () => navTo('/open-capabilities');
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
    <div className='size-full flex flex-col'>
      {/* Main content area */}
      <div className='flex-1 min-h-0 overflow-y-auto overflow-x-hidden'>
        {isSettings ? (
          <Suspense fallback={<div className='size-full' />}>
            <SettingsSider collapsed={collapsed} tooltipEnabled={tooltipEnabled} />
          </Suspense>
        ) : (
          <div className='size-full flex flex-col gap-2px'>
            {/* 常用 — high-frequency primary destinations */}
            <SiderSectionHeader label={t('common.siderSection.common')} collapsed={collapsed} />
            {/* Conversations — opens the session secondary sidebar (ContentSider) */}
            <SiderConversationEntry
              isMobile={isMobile}
              isActive={isSessionRoute}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleConversationClick}
            />
            {isFirstWin ? (
              <button
                type='button'
                className='mx-8px min-h-30px px-10px flex items-center justify-center rd-8px b-none bg-transparent text-t-secondary text-12px cursor-pointer hover:bg-fill-2 hover:text-t-primary'
                aria-expanded={capabilitiesExpanded}
                title={t('common.siderSection.moreCapabilities')}
                onClick={toggleCapabilities}
                data-testid='sider-more-capabilities'
              >
                {collapsed ? '•••' : t('common.siderSection.moreCapabilities')}
              </button>
            ) : null}
            {showCapabilityHub ? (
              <>
            {/* Work partner (桌面伙伴) — demo path 2 */}
            <SiderNomiEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/nomi')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleNomiClick}
            />
            {/* External agents on /mcp-agent — demo path 3 */}
            <SiderOpenCapabilitiesEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/open-capabilities')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleOpenCapabilitiesClick}
            />
            {/* ViMax video generation */}
            <SiderVideoGenerationEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/video-generation')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleVideoGenerationClick}
            />
            {/* 对外服务 — public-facing customer-service agents (对外伙伴), a domain
                fully separate from the desktop-companion group above. */}
            <SiderSectionHeader label={t('common.siderSection.publicService')} collapsed={collapsed} />
            <SiderPublicServiceEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/public-companions')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handlePublicServiceClick}
            />
            {/* 数据空间 — knowledge only; workshop/assets stay deferred */}
            <SiderSectionHeader label={t('common.siderSection.data')} collapsed={collapsed} />
            <SiderKnowledgeEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/knowledge')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleKnowledgeClick}
              dot={pendingInboxCount > 0}
            />
            {/* 自动化 — automation platforms */}
            <SiderSectionHeader label={t('common.siderSection.automation')} collapsed={collapsed} />
            <SiderScheduledEntry
              isMobile={isMobile}
              isActive={pathname === '/scheduled'}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleScheduledClick}
            />
            <SiderRequirementsEntry
              isMobile={isMobile}
              isActive={pathname.startsWith('/requirements')}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              onClick={handleRequirementsClick}
            />
            {/* Config One — presets / skills / MCP as secondary under Config */}
            <SiderSectionHeader label={t('common.siderSection.config', { defaultValue: '配置' })} collapsed={collapsed} />
            <SiderConfigGroup
              isMobile={isMobile}
              collapsed={collapsed}
              siderTooltipProps={siderTooltipProps}
              presetsActive={pathname.startsWith('/presets')}
              skillsActive={pathname.startsWith('/skills')}
              mcpActive={pathname.startsWith('/mcp')}
              onPresets={handlePresetClick}
              onSkills={handleSkillsClick}
              onMcp={handleMcpClick}
            />
              </>
            ) : null}
          </div>
        )}
      </div>
      {/* Bottom pinned group (设置) — Model & Agent and Open Capabilities sit directly above Settings */}
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
