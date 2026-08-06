/**
 * Sider "视频生成" group — WorkpathDrawer-aligned collapsible submenu.
 * Parent click toggles expand/collapse and opens the video-generation home.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { FolderClose, FolderOpen, VideoOne } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { listSessions } from '@renderer/pages/videoGeneration/api';
import {
  RECENT_VIDEO_GENERATION_VISIBLE,
  mergeRecentVideoGenerationProjects,
  readRecentVideoGenerationSessions,
  rememberVideoGenerationSession,
  updateRecentVideoGenerationTitle,
} from '@renderer/pages/videoGeneration/routeMemory';

const NAV_EXPANDED_KEY = 'flowy.videoGeneration.navExpanded';

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
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  /** Open the video-generation home (list). */
  onEnterHome: () => void;
  /** Open a recent project workspace. */
  onOpenProject: (sessionId: string) => void;
}

const SiderVideoGenerationGroup: React.FC<SiderVideoGenerationGroupProps> = ({
  isMobile,
  moduleActive,
  activeSessionId,
  collapsed,
  siderTooltipProps,
  onEnterHome,
  onOpenProject,
}) => {
  const { t } = useTranslation();
  const label = t('videoGeneration.nav.entry', { defaultValue: '视频生成' });
  const [items, setItems] = useState<Array<{ id: string; title: string }>>([]);
  const [expanded, setExpanded] = useState(() => readNavExpandedDefault(true));
  const syncedActiveRouteRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const local = readRecentVideoGenerationSessions();
    try {
      const sessions = await listSessions();
      const merged = mergeRecentVideoGenerationProjects(
        local,
        sessions,
        RECENT_VIDEO_GENERATION_VISIBLE
      );
      setItems((prev) => {
        // Avoid needless re-renders when ids/titles are unchanged (keeps list visually stable).
        if (
          prev.length === merged.length &&
          prev.every((row, i) => row.id === merged[i]?.id && row.title === merged[i]?.title)
        ) {
          return prev;
        }
        return merged;
      });
      for (const row of merged) {
        if (row.title) updateRecentVideoGenerationTitle(row.id, row.title);
      }
    } catch {
      setItems(
        local.slice(0, RECENT_VIDEO_GENERATION_VISIBLE).map((e) => ({
          id: e.id,
          title: e.title?.trim() || '',
        }))
      );
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, activeSessionId]);

  useEffect(() => {
    const onFocus = () => {
      void refresh();
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refresh]);

  // Mirror workpath: when a project route is active, force-expand once per route.
  useEffect(() => {
    if (!activeSessionId) {
      syncedActiveRouteRef.current = null;
      return;
    }
    const routeKey = activeSessionId;
    if (syncedActiveRouteRef.current === routeKey) return;
    syncedActiveRouteRef.current = routeKey;
    setExpanded(true);
    writeNavExpanded(true);
  }, [activeSessionId]);

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
        aria-expanded={expanded}
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
            const active = activeSessionId === item.id;
            return (
              <Tooltip key={item.id} content={fullTitle} position='right'>
                <div
                  role='button'
                  tabIndex={0}
                  data-testid={`sider-video-generation-recent-${item.id}`}
                  title={fullTitle}
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
                    rememberVideoGenerationSession(item.id, fullTitle);
                    onOpenProject(item.id);
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    rememberVideoGenerationSession(item.id, fullTitle);
                    onOpenProject(item.id);
                  }}
                >
                  <span className='chat-history__item-name min-w-0 flex-1 truncate text-14px font-[500] leading-24px text-t-primary'>
                    {short}
                  </span>
                </div>
              </Tooltip>
            );
          })}
        </div>
      ) : null}
    </div>
  );
};

export default SiderVideoGenerationGroup;
