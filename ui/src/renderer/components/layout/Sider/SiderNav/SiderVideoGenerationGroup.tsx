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
  mergeRecentVideoGenerationProjects,
  readRecentVideoGenerationSessions,
  readRecentVideoGenerationTasks,
  rememberVideoGenerationSession,
  updateRecentVideoGenerationTitle,
  type RecentVideoGenerationEntry,
} from '@renderer/pages/videoGeneration/routeMemory';
import { listGenerationTasks } from '@renderer/pages/videoCanvas/api';
import { prefetchVideoGenerationHome } from '@renderer/pages/videoGeneration/prefetch';

const NAV_EXPANDED_KEY = 'flowy.videoGeneration.navExpanded';
/** Refresh frequency while any recent project is actively planning/rendering. */
const ACTIVE_POLL_MS = 4000;

type RecentNavItem = {
  id: string;
  title: string;
  status?: string | null;
  /** Source kind: long-lived session or one-shot clip task. */
  source: 'session' | 'task';
};

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

/** Format a task's prompt into a short sider-friendly title. */
function taskTitleFromPrompt(prompt: string | null | undefined): string {
  return (prompt ?? '').trim().slice(0, 48);
}

function buildFallbackItems(
  localSessions: RecentVideoGenerationEntry[],
  localTasks: RecentVideoGenerationEntry[]
): RecentNavItem[] {
  const items: RecentNavItem[] = [];
  for (const entry of localSessions.slice(0, RECENT_VIDEO_GENERATION_VISIBLE)) {
    items.push({
      id: entry.id,
      title: entry.title?.trim() || '',
      status: null,
      source: 'session',
    });
  }
  for (const entry of localTasks.slice(0, Math.max(0, RECENT_VIDEO_GENERATION_VISIBLE - items.length))) {
    items.push({
      id: entry.id,
      title: entry.title?.trim() || '',
      status: null,
      source: 'task',
    });
  }
  return items.slice(0, RECENT_VIDEO_GENERATION_VISIBLE);
}

function navItemsEqual(prev: RecentNavItem[], next: RecentNavItem[]): boolean {
  if (prev.length !== next.length) return false;
  return prev.every(
    (row, i) =>
      row.id === next[i]?.id &&
      row.title === next[i]?.title &&
      row.source === next[i]?.source &&
      (row.status ?? null) === (next[i]?.status ?? null)
  );
}

type SessionItem = { id: string; title?: string | null; status?: string | null; updated_at?: number | string | null };
type TaskItem = { task_id: string; prompt?: string | null; status?: string | null; updated_at?: number };

/**
 * Merge: local MRU (sessions + tasks) → server sessions → server tasks.
 * Stable insertion order keeps recently-opened items at the top while ensuring
 * historical items the user did not pin still surface up to the visible limit.
 */
function mergeRecentNavItems(
  localSessions: RecentVideoGenerationEntry[],
  localTasks: RecentVideoGenerationEntry[],
  sessions: SessionItem[],
  tasks: TaskItem[],
  limit: number
): RecentNavItem[] {
  const sessionById = new Map(
    sessions.map((s) => [
      s.id,
      {
        title: (s.title ?? '').trim() || '',
        status: s.status ?? null,
        source: 'session' as const,
        updatedAt: typeof s.updated_at === 'number' ? s.updated_at : 0,
      },
    ])
  );
  const taskById = new Map(
    tasks.map((t) => [
      t.task_id,
      {
        title: taskTitleFromPrompt(t.prompt),
        status: t.status ?? null,
        source: 'task' as const,
        updatedAt: t.updated_at ?? 0,
      },
    ])
  );

  const out: RecentNavItem[] = [];
  const used = new Set<string>();

  const pushIfKnown = (id: string, cachedTitle?: string) => {
    if (out.length >= limit || used.has(id)) return;
    const sessionHit = sessionById.get(id);
    if (sessionHit) {
      used.add(id);
      out.push({
        id,
        title: sessionHit.title || cachedTitle || '',
        status: sessionHit.status,
        source: 'session',
      });
      return;
    }
    const taskHit = taskById.get(id);
    if (taskHit) {
      used.add(id);
      out.push({
        id,
        title: taskHit.title || cachedTitle || '',
        status: taskHit.status,
        source: 'task',
      });
    }
  };

  for (const entry of localSessions) pushIfKnown(entry.id, entry.title);
  for (const entry of localTasks) pushIfKnown(entry.id, entry.title);

  // Fill with remaining server-side entries ordered by recency (the server
  // returns them most-recent-first; fall back to a stable sort if missing).
  const sessionRest = [...sessions]
    .filter((s) => !used.has(s.id))
    .sort((a, b) => {
      const av = typeof a.updated_at === 'number' ? a.updated_at : 0;
      const bv = typeof b.updated_at === 'number' ? b.updated_at : 0;
      return bv - av;
    });
  for (const s of sessionRest) {
    if (out.length >= limit) break;
    used.add(s.id);
    out.push({
      id: s.id,
      title: (s.title ?? '').trim(),
      status: s.status ?? null,
      source: 'session',
    });
  }

  const taskRest = [...tasks]
    .filter((t) => !used.has(t.task_id))
    .sort((a, b) => {
      const av = typeof a.updated_at === 'number' ? a.updated_at : 0;
      const bv = typeof b.updated_at === 'number' ? b.updated_at : 0;
      return bv - av;
    });
  for (const t of taskRest) {
    if (out.length >= limit) break;
    used.add(t.task_id);
    out.push({
      id: t.task_id,
      title: taskTitleFromPrompt(t.prompt),
      status: t.status ?? null,
      source: 'task',
    });
  }

  return out;
}

export interface SiderVideoGenerationGroupProps {
  isMobile: boolean;
  /** True when pathname is under `/video-generation`. */
  moduleActive: boolean;
  /** Current workspace session id, if any. */
  activeSessionId: string | null;
  /** Current clip task id, if any. */
  activeClipTaskId: string | null;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  /** Open the video-generation home (list). */
  onEnterHome: () => void;
  /** Open a recent project workspace. */
  onOpenProject: (sessionId: string) => void;
  /** Open a recent clip task. */
  onOpenClipTask: (taskId: string) => void;
}

const SiderVideoGenerationGroup: React.FC<SiderVideoGenerationGroupProps> = ({
  isMobile,
  moduleActive,
  activeSessionId,
  activeClipTaskId,
  collapsed,
  siderTooltipProps,
  onEnterHome,
  onOpenProject,
  onOpenClipTask,
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
      const [sessions, taskList] = await Promise.all([
        listSessions().catch(() => []),
        listGenerationTasks(20, 0).then((r) => r.tasks).catch(() => []),
      ]);
      const merged = mergeRecentNavItems(
        localSessions,
        localTasks,
        sessions,
        taskList,
        RECENT_VIDEO_GENERATION_VISIBLE
      );
      setItems((prev) => {
        if (navItemsEqual(prev, merged)) return prev;
        return merged;
      });
      for (const row of merged) {
        if (row.title) updateRecentVideoGenerationTitle(row.id, row.title);
      }
    } catch {
      setItems(buildFallbackItems(localSessions, localTasks));
    }
  }, []);

  const hasActiveRecent = useMemo(
    () => items.some((item) => isActiveStatus(item.status)),
    [items]
  );

  useEffect(() => {
    void refresh();
  }, [refresh, activeSessionId]);

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
    const routeKey = activeSessionId ?? activeClipTaskId;
    if (!routeKey) {
      syncedActiveRouteRef.current = null;
      return;
    }
    if (syncedActiveRouteRef.current === routeKey) return;
    syncedActiveRouteRef.current = routeKey;
    setExpanded(true);
    writeNavExpanded(true);
  }, [activeSessionId, activeClipTaskId]);

  const handleParentClick = useCallback(() => {
    setExpanded((prev) => {
      const next = !prev;
      writeNavExpanded(next);
      return next;
    });
    onEnterHome();
  }, [onEnterHome]);

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

  const showChildren = expanded && items.length > 0;

  return (
    <div className='workpath-drawer min-w-0' data-testid='sider-video-generation-group'>
      <div
        className={classNames(
          'flex items-center gap-8px pl-10px pr-8px h-34px cursor-pointer hover:bg-fill-2 rd-10px transition-colors min-w-0 text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          moduleActive && !activeSessionId ? '!bg-primary-1 !text-primary-6' : ''
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
                      : '';
            const isTask = item.source === 'task';
            const openItem = () => {
              if (isTask) {
                rememberVideoGenerationSession(item.id, fullTitle);
                onOpenClipTask(item.id);
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
