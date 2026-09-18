/**
 * Sider "视频生成" group — WorkpathDrawer-aligned collapsible submenu.
 * Parent click toggles expand/collapse and opens the video-generation home.
 * Recent rows show a spinning loader when planning / rendering.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popover, Tooltip } from '@arco-design/web-react';
import { FolderClose, FolderOpen, Loading, VideoOne } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { isActiveStatus, listSessions } from '@renderer/pages/videoGeneration/api';
import VideoGenerationHoverCard from '@renderer/pages/videoGeneration/components/VideoGenerationHoverCard';
import {
  RECENT_VIDEO_GENERATION_VISIBLE,
  readRecentVideoGenerationSessions,
  readRecentVideoGenerationTasks,
  rememberVideoGenerationBriefing,
  rememberVideoGenerationCanvas,
  rememberVideoGenerationSession,
  rememberVideoGenerationTask,
  updateRecentVideoGenerationTitle,
} from '@renderer/pages/videoGeneration/routeMemory';
import {
  fallbackNavItems,
  mergeRecentNavItems,
  navItemsEqual,
  type RecentCreationKind,
  type RecentNavItem,
} from '@renderer/pages/videoGeneration/recentCreations';
import { listBriefingSessions } from '@renderer/pages/videoGeneration/briefing/api';
import { listCanvasProjects, listGenerationTasks } from '@renderer/pages/videoCanvas/api';
import { prefetchVideoGenerationHome } from '@renderer/pages/videoGeneration/prefetch';

const NAV_EXPANDED_KEY = 'flowy.videoGeneration.navExpanded';
/** Refresh frequency while any recent project is actively planning/rendering. */
const ACTIVE_POLL_MS = 4000;

function readNavExpandedDefault(fallback: boolean): boolean {
  try {
    const raw = window.localStorage.getItem(NAV_EXPANDED_KEY);
    if (raw === '1' || raw === 'true') return true;
    if (raw === '0' || raw === 'false') return false;
  } catch {
    /* ignore */
  }
  return fallback;
}

function writeNavExpanded(expanded: boolean): void {
  try {
    window.localStorage.setItem(NAV_EXPANDED_KEY, expanded ? '1' : '0');
  } catch {
    /* ignore */
  }
}

function truncateTitle(title: string, maxChars = 18): string {
  const t = title.trim();
  if (!t) return '';
  if ([...t].length <= maxChars) return t;
  return `${[...t].slice(0, maxChars - 1).join('')}…`;
}

export interface SiderVideoGenerationGroupProps {
  isMobile: boolean;
  /** True when pathname is under `/video-generation`. */
  moduleActive: boolean;
  /** Current workspace session id, if any. */
  activeSessionId: string | null;
  /** Current clip task id, if any. */
  activeClipTaskId: string | null;
  /** Current canvas project id, if any. */
  activeCanvasProjectId: string | null;
  /** Current briefing session id, if any. */
  activeBriefingId: string | null;
  collapsed: boolean;
  dock?: boolean;
  flat?: boolean;
  siderTooltipProps: SiderTooltipProps;
  /** Open the video-generation home (list). */
  onEnterHome: () => void;
  /** Open a recent agent/action workspace. */
  onOpenProject: (sessionId: string) => void;
  /** Open a recent clip task. */
  onOpenClipTask: (taskId: string) => void;
  /** Open a recent infinite-canvas project. */
  onOpenCanvasProject: (projectId: string) => void;
  /** Open a recent briefing workspace. */
  onOpenBriefing: (briefingId: string) => void;
}

const SiderVideoGenerationGroup: React.FC<SiderVideoGenerationGroupProps> = ({
  isMobile,
  moduleActive,
  activeSessionId,
  activeClipTaskId,
  activeCanvasProjectId,
  activeBriefingId,
  collapsed,
  dock = false,
  flat = false,
  siderTooltipProps,
  onEnterHome,
  onOpenProject,
  onOpenClipTask,
  onOpenCanvasProject,
  onOpenBriefing,
}) => {
  const { t } = useTranslation();
  const label = t('videoGeneration.nav.entry', { defaultValue: '视频生成' });
  const [items, setItems] = useState<RecentNavItem[]>([]);
  const [expanded, setExpanded] = useState(() => readNavExpandedDefault(true));
  const syncedActiveRouteRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const localSessions = readRecentVideoGenerationSessions();
    const localTasks = readRecentVideoGenerationTasks();
    try {
      const [sessionsResult, taskResult, canvasResult, briefingResult] = await Promise.allSettled([
        listSessions(),
        listGenerationTasks(20, 0, { standalone: true }).then((r) => r.tasks),
        listCanvasProjects(),
        listBriefingSessions(),
      ]);
      const sessions = sessionsResult.status === 'fulfilled' ? sessionsResult.value : [];
      const taskList = taskResult.status === 'fulfilled' ? taskResult.value : [];
      const canvases = canvasResult.status === 'fulfilled' ? canvasResult.value : [];
      const briefings = briefingResult.status === 'fulfilled' ? briefingResult.value : [];
      const loadedKinds = new Set<RecentCreationKind>();
      if (sessionsResult.status === 'fulfilled') loadedKinds.add('session');
      if (taskResult.status === 'fulfilled') loadedKinds.add('task');
      if (canvasResult.status === 'fulfilled') loadedKinds.add('canvas');
      if (briefingResult.status === 'fulfilled') loadedKinds.add('briefing');
      const merged = mergeRecentNavItems(
        localSessions,
        localTasks,
        sessions,
        taskList,
        canvases,
        briefings,
        RECENT_VIDEO_GENERATION_VISIBLE,
        loadedKinds
      );
      setItems((prev) => {
        if (navItemsEqual(prev, merged)) return prev;
        return merged;
      });
      for (const row of merged) {
        if (row.title) updateRecentVideoGenerationTitle(row.id, row.title);
      }
    } catch {
      setItems(fallbackNavItems(localSessions, localTasks, RECENT_VIDEO_GENERATION_VISIBLE));
    }
  }, []);

  const hasActiveRecent = useMemo(
    () => items.some((item) => isActiveStatus(item.status)),
    [items]
  );

  useEffect(() => {
    void refresh();
  }, [refresh, activeSessionId, activeClipTaskId, activeCanvasProjectId, activeBriefingId]);

  useEffect(() => {
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(() => prefetchVideoGenerationHome(), {
        timeout: 1800,
      });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(() => prefetchVideoGenerationHome(), 250);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    const onFocus = () => {
      void refresh();
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refresh]);

  // Keep sider spinners in sync while any recent project is planning/rendering.
  useEffect(() => {
    if (!hasActiveRecent) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, ACTIVE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [hasActiveRecent, refresh]);

  // Mirror workpath: when a project route is active, force-expand once per route.
  useEffect(() => {
    const routeKey =
      activeSessionId ?? activeClipTaskId ?? activeCanvasProjectId ?? activeBriefingId;
    if (!routeKey) {
      syncedActiveRouteRef.current = null;
      return;
    }
    if (syncedActiveRouteRef.current === routeKey) return;
    syncedActiveRouteRef.current = routeKey;
    setExpanded(true);
    writeNavExpanded(true);
  }, [activeSessionId, activeClipTaskId, activeCanvasProjectId, activeBriefingId]);

  const handleParentClick = useCallback(() => {
    setExpanded((prev) => {
      const next = !prev;
      writeNavExpanded(next);
      return next;
    });
    onEnterHome();
  }, [onEnterHome]);

  if (dock) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='bottom'>
        <div
          className={classNames(
            'size-26px flex items-center justify-center cursor-pointer transition-colors rd-6px text-t-secondary hover:text-t-primary relative',
            moduleActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onEnterHome}
          onPointerEnter={() => prefetchVideoGenerationHome()}
          aria-current={moduleActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={moduleActive ? 'true' : 'false'}
          data-sider-selection-static='true'
        >
          <VideoOne
            theme='outline'
            size='15'
            fill='currentColor'
            className='block leading-none shrink-0'
            style={{ lineHeight: 0 }}
          />
          {items.length > 0 && (
            <span
              className='absolute -top-1px -right-1px min-w-10px h-10px px-2px bg-primary text-white text-8px rd-full flex items-center justify-center font-bold leading-none select-none'
              style={{ fontSize: 8 }}
            >
              {items.length}
            </span>
          )}
        </div>
      </Tooltip>
    );
  }

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <div
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary',
            moduleActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onEnterHome}
          onPointerEnter={() => prefetchVideoGenerationHome()}
          aria-current={moduleActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={moduleActive ? 'true' : 'false'}
          data-testid='sider-video-generation-collapsed'
        >
          <VideoOne
            theme='outline'
            size='20'
            fill='currentColor'
            className='block leading-none shrink-0'
            style={{ lineHeight: 0 }}
          />
        </div>
      </Tooltip>
    );
  }

  if (flat) {
    return (
      <div className='flex flex-col min-w-0' data-testid='sider-video-generation-group'>
        <div className='flex items-center justify-between px-6px py-4px mb-2px'>
          <span className='text-11px font-[500] text-t-tertiary select-none'>
            {t('videoGeneration.nav.recentCreations', { defaultValue: '最近创作' })}
          </span>
          {items.length > 0 ? (
            <span className='text-11px text-t-tertiary tabular-nums select-none'>{items.length}</span>
          ) : null}
        </div>

        {items.length === 0 ? (
          <div className='flex flex-col items-center justify-center py-24px px-8px text-center gap-8px'>
            <span className='text-12px text-t-tertiary'>
              {t('videoGeneration.list.empty', { defaultValue: '暂无创作记录' })}
            </span>
            <button
              type='button'
              onClick={onEnterHome}
              onPointerEnter={() => prefetchVideoGenerationHome()}
              className='h-26px px-10px rd-6px text-11px font-[500] bg-fill-2 hover:bg-fill-3 active:bg-fill-4 text-t-primary border border-solid border-[var(--color-border-2)] cursor-pointer transition-colors select-none'
            >
              + {t('videoGeneration.nav.newProject', { defaultValue: '新建视频工程' })}
            </button>
          </div>
        ) : (
          <div
            className='flex flex-col gap-2px'
            data-testid='sider-video-generation-recents'
          >
            {items.map((item) => {
              const fullTitle =
                item.title.trim() ||
                t('videoGeneration.list.untitled', { defaultValue: '未命名任务' });
              const short = truncateTitle(fullTitle, 16);
              const active =
                item.source === 'task'
                  ? activeClipTaskId === item.id
                  : item.source === 'canvas'
                    ? activeCanvasProjectId === item.id
                    : item.source === 'briefing'
                      ? activeBriefingId === item.id
                      : activeSessionId === item.id;
              const busy = isActiveStatus(item.status);
              const busyHint =
                item.status === 'planning'
                  ? t('videoGeneration.status.planning', { defaultValue: '规划中' })
                  : item.status === 'rendering'
                    ? t('videoGeneration.status.rendering', { defaultValue: '生成中' })
                    : item.status === 'queued'
                      ? t('videoGeneration.clip.status.queued', { defaultValue: '排队中' })
                      : item.status === 'running'
                        ? t('videoGeneration.clip.status.running', { defaultValue: '生成中' })
                        : item.status === 'researching' ||
                            item.status === 'scripting' ||
                            item.status === 'aligning' ||
                            item.status === 'composing'
                          ? t('videoGeneration.briefing.runningTitle', { defaultValue: '生成中' })
                          : '';
              const openItem = () => {
                if (item.source === 'task') {
                  rememberVideoGenerationTask(item.id, fullTitle);
                  onOpenClipTask(item.id);
                } else if (item.source === 'canvas') {
                  rememberVideoGenerationCanvas(item.id, fullTitle);
                  onOpenCanvasProject(item.id);
                } else if (item.source === 'briefing') {
                  rememberVideoGenerationBriefing(item.id, fullTitle);
                  onOpenBriefing(item.id);
                } else {
                  rememberVideoGenerationSession(item.id, fullTitle);
                  onOpenProject(item.id);
                }
              };
              const row = (
                <div
                  role='button'
                  tabIndex={0}
                  data-testid={`sider-video-generation-recent-${item.id}`}
                  data-busy={busy ? 'true' : 'false'}
                  data-source={item.source}
                  className={classNames(
                    'chat-history__item conversation-item h-34px rd-8px flex items-center group cursor-pointer relative overflow-hidden shrink-0 min-w-0 transition-colors justify-start gap-8px px-8px',
                    isMobile && 'sider-action-btn-mobile',
                    {
                      'hover:bg-fill-2': !active,
                      'session-list-active-row !text-t-primary !bg-fill-3 font-semibold': active,
                    }
                  )}
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    openItem();
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    openItem();
                  }}
                >
                  <span
                    className={classNames(
                      'size-18px flex items-center justify-center shrink-0 text-t-secondary transition-colors',
                      active && '!text-primary-6'
                    )}
                  >
                    <VideoOne
                      theme='outline'
                      size={14}
                      fill='currentColor'
                      className='block leading-none'
                      style={{ lineHeight: 0 }}
                    />
                  </span>
                  <span className='chat-history__item-name min-w-0 flex-1 truncate text-13px font-[450] leading-20px text-t-primary text-left'>
                    {short}
                  </span>
                  {busy ? (
                    <span className='shrink-0 flex items-center gap-2px text-10px text-primary-6 bg-primary-1 px-4px py-1px rd-4px font-normal'>
                      <Loading
                        theme='outline'
                        size={11}
                        fill='currentColor'
                        className='block shrink-0 animate-spin text-[rgb(var(--primary-6))]'
                        style={{ lineHeight: 0 }}
                        aria-label={busyHint}
                      />
                      <span>{busyHint || t('videoGeneration.status.rendering', { defaultValue: '生成中' })}</span>
                    </span>
                  ) : null}
                </div>
              );
              return (
                <Popover
                  key={item.id}
                  trigger='hover'
                  position='right'
                  content={
                    <VideoGenerationHoverCard
                      id={item.id}
                      title={fullTitle}
                      status={item.status}
                    />
                  }
                  triggerProps={{ mouseEnterDelay: 400 }}
                >
                  {row}
                </Popover>
              );
            })}
          </div>
        )}
      </div>
    );
  }

  const showChildren = expanded && items.length > 0;

  return (
    <div className='workpath-drawer min-w-0' data-testid='sider-video-generation-group'>
      <div
        className={classNames(
          'flex items-center gap-8px pl-10px pr-8px h-34px cursor-pointer hover:bg-fill-2 rd-10px transition-colors min-w-0 text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          moduleActive &&
            !activeSessionId &&
            !activeClipTaskId &&
            !activeCanvasProjectId &&
            !activeBriefingId
            ? '!bg-primary-1 !text-primary-6'
            : ''
        )}
        onClick={handleParentClick}
        onPointerEnter={() => prefetchVideoGenerationHome()}
        aria-expanded={expanded}
        aria-current={moduleActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={moduleActive ? 'true' : 'false'}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          {expanded ? (
            <FolderOpen
              theme='outline'
              size={16}
              fill='currentColor'
              className='block leading-none'
              style={{ lineHeight: 0 }}
            />
          ) : (
            <FolderClose
              theme='outline'
              size={16}
              fill='currentColor'
              className='block leading-none'
              style={{ lineHeight: 0 }}
            />
          )}
        </span>

        <span className='flex-1 min-w-0 text-14px font-[500] leading-24px truncate text-left'>
          {label}
        </span>

        {items.length > 0 ? (
          <span className='shrink-0 text-11px text-t-tertiary tabular-nums'>{items.length}</span>
        ) : null}
      </div>

      {showChildren ? (
        <div
          className='workpath-drawer-content flex flex-col pt-2px'
          data-testid='sider-video-generation-recents'
        >
          {items.map((item) => {
            const fullTitle =
              item.title.trim() ||
              t('videoGeneration.list.untitled', { defaultValue: '未命名任务' });
            const short = truncateTitle(fullTitle);
            const active =
              item.source === 'task'
                ? activeClipTaskId === item.id
                : item.source === 'canvas'
                  ? activeCanvasProjectId === item.id
                  : item.source === 'briefing'
                    ? activeBriefingId === item.id
                    : activeSessionId === item.id;
            const busy = isActiveStatus(item.status);
            const busyHint =
              item.status === 'planning'
                ? t('videoGeneration.status.planning', { defaultValue: '规划中' })
                : item.status === 'rendering'
                  ? t('videoGeneration.status.rendering', { defaultValue: '生成中' })
                  : item.status === 'queued'
                    ? t('videoGeneration.clip.status.queued', { defaultValue: '排队中' })
                    : item.status === 'running'
                      ? t('videoGeneration.clip.status.running', { defaultValue: '生成中' })
                      : item.status === 'researching' ||
                          item.status === 'scripting' ||
                          item.status === 'aligning' ||
                          item.status === 'composing'
                        ? t('videoGeneration.briefing.runningTitle', { defaultValue: '生成中' })
                        : '';
            const openItem = () => {
              if (item.source === 'task') {
                rememberVideoGenerationTask(item.id, fullTitle);
                onOpenClipTask(item.id);
              } else if (item.source === 'canvas') {
                rememberVideoGenerationCanvas(item.id, fullTitle);
                onOpenCanvasProject(item.id);
              } else if (item.source === 'briefing') {
                rememberVideoGenerationBriefing(item.id, fullTitle);
                onOpenBriefing(item.id);
              } else {
                rememberVideoGenerationSession(item.id, fullTitle);
                onOpenProject(item.id);
              }
            };
            const row = (
                <div
                  role='button'
                  tabIndex={0}
                  data-testid={`sider-video-generation-recent-${item.id}`}
                  data-busy={busy ? 'true' : 'false'}
                  data-source={item.source}
                  className={classNames(
                    // Match ConversationRow (dimIcon) — use div, not <button>, to avoid UA black borders.
                    'chat-history__item conversation-item h-34px rd-10px flex items-center group cursor-pointer relative overflow-hidden shrink-0 min-w-0 transition-colors justify-start gap-8px pl-42px pr-16px',
                    '[&.conversation-item+&.conversation-item]:mt-2px',
                    isMobile && 'sider-action-btn-mobile',
                    {
                      'hover:bg-fill-2': !active,
                      'session-list-active-row !text-t-primary': active,
                    }
                  )}
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    openItem();
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    openItem();
                  }}
                >
                  {busy ? (
                    <Loading
                      theme='outline'
                      size={14}
                      fill='currentColor'
                      className='block shrink-0 animate-spin text-[rgb(var(--primary-6))]'
                      style={{ lineHeight: 0 }}
                      aria-label={busyHint}
                    />
                  ) : null}
                  <span className='chat-history__item-name min-w-0 flex-1 truncate text-14px font-[500] leading-24px text-t-primary'>
                    {short}
                  </span>
                </div>
            );
            return (
              <Popover
                key={item.id}
                trigger='hover'
                position='right'
                content={
                  <VideoGenerationHoverCard
                    id={item.id}
                    title={fullTitle}
                    status={item.status}
                  />
                }
                triggerProps={{ mouseEnterDelay: 400 }}
              >
                {row}
              </Popover>
            );
          })}
        </div>
      ) : null}
    </div>
  );
};

export default SiderVideoGenerationGroup;
