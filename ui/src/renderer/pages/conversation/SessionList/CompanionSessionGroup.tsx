

import { ipcBridge } from '@/common';
import type { ICompanionWithStatus } from '@/common/adapter/ipcBridge';
import type { TChatConversation } from '@/common/config/storage';
import type { ConversationId } from '@/common/types/ids';
import CompanionAvatar from '@renderer/pages/companion/CompanionAvatar';
import type { CompanionMood } from '@renderer/pages/companion/characters';
import { customFigureMetaOf } from '@renderer/pages/companion/characters/customMeta';
import { useCompanions } from '@renderer/pages/nomi/useNomi';
import { cleanupSiderTooltips } from '@renderer/utils/ui/siderTooltip';
import { Message, Tooltip } from '@arco-design/web-react';
import { Attention, Robot } from '@icon-park/react';
import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import useSWR from 'swr';

import {
  COMPANION_COLLAPSED_LIST_LIMIT,
  getVisibleCompanionEntries,
} from './utils/companionVisibleEntries';
import { useConversationListSync } from './hooks/useConversationListSync';
import { useDisclosureMotion } from './hooks/useDisclosureMotion';
import SessionOverflowButton from './SessionOverflowButton';

interface Props {
  /** Active conversation id parsed from the `/conversation/:id` route, for row highlight. */
  activeConversationId: ConversationId | null;
  /** Icon-only rail variant (parent sider collapsed). */
  collapsed?: boolean;
  /** Closes the mobile drawer / clears tooltips after navigating, mirrors the workpath list. */
  onSessionClick?: () => void;
  /** Fold state of the group (persisted in useWorkpathUiState). Ignored in the collapsed rail. */
  expanded?: boolean;
  /** Toggles the persisted fold state. */
  onToggleExpanded?: () => void;
}

const modelReadyOf = (c: ICompanionWithStatus) => Boolean(c.model?.provider_id && c.model?.model);

/**
 * 会话侧边栏顶部的「桌面伙伴」专属工作空间分组（roster-driven）。
 *
 * 把伙伴聊天迁进「会话」：数据源是伙伴花名册（useCompanions），每个伙伴 = 一行 =
 * 其唯一专属会话（单会话契约）。点击行解析（幂等 ensure）该伙伴的会话并跳转标准
 * `/conversation/:id`（由 ChatConversation 识别 extra.companionSession 渲染受限聊天）。
 *
 * 与项目/工作路径分组的区别：仅交互式会话（无终端子组）、不在此新建（创建仍在管理中心
 * /nomi）。未配置模型的伙伴点击跳转管理中心引导配置，而非创建会话（后端会 400）。
 *
 * 不触碰工作会话过滤器：伙伴会话仍被 useConversationListSync 过滤出项目分组，故不会重复列出。
 */
const CompanionSessionGroup: React.FC<Props> = ({
  activeConversationId,
  collapsed = false,
  onSessionClick,
  expanded = true,
  onToggleExpanded,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { companions } = useCompanions();
  const [showAllCompanions, setShowAllCompanions] = useState(false);
  const [groupToggleKey, setGroupToggleKey] = useState(0);
  const [overflowToggleKey, setOverflowToggleKey] = useState(0);
  const groupMotion = useDisclosureMotion(expanded, groupToggleKey);

  const handleGroupToggle = () => {
    setGroupToggleKey((value) => value + 1);
    onToggleExpanded?.();
  };

  // companionId → 其唯一会话 id（只读解析，用于活动行高亮 + 点击直达，避免无谓 ensure）。
  // 随花名册变化重解析；getCompanionSession 对未建会话返回 null（不入表）。
  const [sessionMap, setSessionMap] = useState<Map<string, ConversationId>>(new Map());
  const rosterKey = companions.map((c) => c.companion_id).join(',');
  useEffect(() => {
    if (companions.length === 0) {
      setSessionMap((prev) => (prev.size === 0 ? prev : new Map()));
      return;
    }
    let cancelled = false;
    void Promise.all(
      companions.map(async (c) => {
        try {
          const r = await ipcBridge.companion.getCompanionSession.invoke({
            companion_id: c.companion_id,
          });
          return [c.companion_id, r.conversation_id] as const;
        } catch {
          return [c.companion_id, null] as const;
        }
      })
    ).then((entries) => {
      if (cancelled) return;
      const next = new Map<string, ConversationId>();
      for (const [id, cid] of entries) if (cid != null) next.set(id, cid);
      setSessionMap(next);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rosterKey]);

  const handleOpen = useCallback(
    async (c: ICompanionWithStatus) => {
      cleanupSiderTooltips();
      onSessionClick?.();
      const cached = sessionMap.get(c.companion_id);
      if (cached != null) {
        void navigate(`/conversation/${cached}`);
        return;
      }
      // 未配置模型：无法 ensure（后端 400）→ 跳管理中心引导配置。
      if (!modelReadyOf(c)) {
        void navigate(
          `/nomi?companion=${encodeURIComponent(c.companion_id)}&tab=overview`
        );
        return;
      }
      try {
        const thread = await ipcBridge.companion.ensureCompanionSession.invoke({
          companion_id: c.companion_id,
        });
        setSessionMap((prev) =>
          new Map(prev).set(c.companion_id, thread.conversation_id)
        );
        void navigate(`/conversation/${thread.conversation_id}`);
      } catch (e) {
        Message.error(String(e));
      }
    },
    [navigate, onSessionClick, sessionMap]
  );

  // Robot threads bound to each companion. A device's chat belongs to its 伙伴,
  // shown nested under that companion row rather than in a separate top-level
  // bucket — the robot thread stays distinct (its own voice-tuned prompt), it is
  // just attributed to its companion here.
  const { robotConversations } = useConversationListSync();
  const { data: robots } = useSWR('robots.list', () => ipcBridge.robot.list.invoke());
  const robotNames = useMemo(() => {
    const names = new Map<string, string>();
    for (const r of robots ?? []) names.set(r.robot_id, r.name);
    return names;
  }, [robots]);
  const robotsByCompanion = useMemo(() => {
    const byCompanion = new Map<string, TChatConversation[]>();
    for (const conv of robotConversations) {
      const companionId = (conv.extra as { companion_id?: string } | undefined)?.companion_id;
      if (companionId == null) continue;
      const bucket = byCompanion.get(companionId);
      if (bucket) bucket.push(conv);
      else byCompanion.set(companionId, [conv]);
    }
    return byCompanion;
  }, [robotConversations]);

  const openRobotConversation = useCallback(
    (id: ConversationId) => {
      cleanupSiderTooltips();
      onSessionClick?.();
      void navigate(`/conversation/${id}`);
    },
    [navigate, onSessionClick]
  );

  const activeCompanionIndex =
    activeConversationId == null
      ? -1
      : companions.findIndex(
          (c) => sessionMap.get(c.companion_id) === activeConversationId
        );
  const forceShowActiveCompanion =
    activeCompanionIndex >= COMPANION_COLLAPSED_LIST_LIMIT;
  const overflowMotion = useDisclosureMotion(
    showAllCompanions || forceShowActiveCompanion,
    overflowToggleKey
  );

  // 无伙伴时不渲染分组（避免对不使用伙伴的用户造成噪音；创建后经 WS 刷新即出现）。
  if (companions.length === 0) return null;

  if (collapsed) {
    return (
      <div className='min-w-0 flex flex-col items-center gap-4px mb-4px'>
        {companions.map((c) => {
          const active =
            activeConversationId != null &&
            sessionMap.get(c.companion_id) === activeConversationId;
          return (
            <Tooltip key={c.companion_id} content={c.name} position='right' mini>
              <div
                role='button'
                aria-label={c.name}
                onClick={() => void handleOpen(c)}
                className={classNames(
                  'flex items-center justify-center w-36px h-36px rd-10px cursor-pointer transition-colors',
                  active ? '!bg-primary-1' : 'hover:bg-fill-2 active:bg-fill-3'
                )}
              >
                <CompanionAvatar
                  character={c.character}
                  companionId={c.companion_id}
                  customFigure={customFigureMetaOf(c)}
                  mood={(c.status.mood as CompanionMood) || 'content'}
                  activity='idle'
                  size={28}
                />
              </div>
            </Tooltip>
          );
        })}
      </div>
    );
  }

  const visibleCompanions = getVisibleCompanionEntries(
    companions,
    showAllCompanions || forceShowActiveCompanion
  );
  const companionOverflowCount = Math.max(0, companions.length - COMPANION_COLLAPSED_LIST_LIMIT);
  const baseCompanions = companions.slice(0, COMPANION_COLLAPSED_LIST_LIMIT);
  const overflowCompanions = companions.slice(COMPANION_COLLAPSED_LIST_LIMIT);

  const toggleOverflow = () => {
    setOverflowToggleKey((value) => value + 1);
    setShowAllCompanions((value) => !value);
  };

  const renderCompanion = (c: ICompanionWithStatus) => {
    const active =
      activeConversationId != null &&
      sessionMap.get(c.companion_id) === activeConversationId;
    const modelReady = modelReadyOf(c);
    const companionRobots = robotsByCompanion.get(c.companion_id) ?? [];

    return (
      <React.Fragment key={c.companion_id}>
        <div
          onClick={() => void handleOpen(c)}
          className={classNames(
            'group flex items-center gap-8px shrink-0 rd-10px pl-10px pr-8px h-34px cursor-pointer transition-colors box-border min-w-0',
            active ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
        >
          <div className='relative size-22px shrink-0 flex items-center justify-center'>
            <CompanionAvatar
              character={c.character}
              companionId={c.companion_id}
              customFigure={customFigureMetaOf(c)}
              mood={(c.status.mood as CompanionMood) || 'content'}
              activity='idle'
              size={20}
            />
            {/* 这圈边框把状态点从头像上「抠」出来，所以宽度和样式都得写实：
                `border-2` 只是 --bg-2 颜色，没有宽度也没有 border-style（本仓库
                没有全局 border reset），整圈一个像素都不画。
                The cut-out ring needs a real width AND a style to exist. */}
            <span
              className='absolute -right-1px -bottom-1px w-9px h-9px rd-full border-2px border-solid border-[var(--color-bg-1)]'
              style={{ background: modelReady ? 'rgb(var(--success-6))' : 'rgb(var(--warning-6))' }}
              title={modelReady ? undefined : t('nomi.chat.modelUnset')}
            />
          </div>
          <div className='flex flex-col gap-1px min-w-0 flex-1'>
            <span
              className={classNames(
                'text-13px font-600 leading-16px truncate min-w-0',
                active ? '!text-primary-6' : 'text-t-primary'
              )}
            >
              {c.name}
            </span>
            <span className={classNames('text-11px leading-13px', active ? 'text-primary-6 opacity-70' : 'text-t-tertiary')}>
              Lv{c.status.level}
            </span>
          </div>
        </div>
        {/* 机器人对话：归属到该伙伴之下，缩进为二级条目。点击直达其会话
            （ChatConversation 识别 type='nomi'+companion_session 渲染）。 */}
        {companionRobots.map((conv) => {
          const robotId = (conv.extra as { robot_id?: string } | undefined)?.robot_id ?? '';
          const label = robotNames.get(robotId) ?? t('nomi.robot.group.deviceUnknown');
          const activeRobot = activeConversationId != null && conv.id === activeConversationId;
          return (
            <Tooltip key={conv.id} content={t('nomi.robot.group.deviceTooltip', { robot: label })} position='right' mini>
              <div
                onClick={(e) => {
                  e.stopPropagation();
                  openRobotConversation(conv.id);
                }}
                className={classNames(
                  'flex items-center gap-6px shrink-0 rd-8px pl-42px pr-8px py-3px cursor-pointer transition-colors box-border min-w-0',
                  activeRobot ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
                )}
              >
                <span className='size-16px flex items-center justify-center shrink-0 text-t-tertiary'>
                  <Robot theme='outline' size={12} fill='currentColor' className='block leading-none' />
                </span>
                <span
                  className={classNames(
                    'text-12px truncate min-w-0',
                    activeRobot ? '!text-primary-6' : 'text-t-secondary'
                  )}
                >
                  {label}
                </span>
              </div>
            </Tooltip>
          );
        })}
      </React.Fragment>
    );
  };

  return (
    <div className='min-w-0 mb-2px'>
      {/* 感叹号跟在标题后；黑框提示出现在图标正下方。 */}
      <div className='pl-10px pr-4px pb-6px flex items-center justify-between gap-8px min-w-0'>
        <div className='flex items-center gap-4px min-w-0'>
          <button
            type='button'
            aria-expanded={expanded}
            aria-controls='flowy-companion-sessions'
            className='sider-section-title appearance-none border-none bg-transparent p-0 text-13px font-[500] leading-none tracking-wide truncate shrink-0 opacity-75 transition-opacity hover:opacity-100 cursor-pointer'
            onClick={handleGroupToggle}
          >
            {t('sessionList.companionGroup')}
          </button>
          <Tooltip
            content={t('sessionList.companionTip')}
            position='bottom'
            mini
            className='sider-tooltip-popup'
            unmountOnExit
          >
            <span
              role='img'
              aria-label={t('sessionList.companionTip')}
              className='inline-flex size-14px shrink-0 items-center justify-center text-t-tertiary opacity-75 hover:opacity-100 cursor-default'
              onClick={(e) => e.stopPropagation()}
            >
              <Attention theme='outline' size={12} fill='currentColor' className='block leading-none' />
            </span>
          </Tooltip>
        </div>
        <span className='text-12px text-t-tertiary leading-none shrink-0'>{companions.length}</span>
      </div>

      {groupMotion.shouldRender && (
        <div
          id='flowy-companion-sessions'
          aria-hidden={groupMotion.phase === 'exiting'}
          data-disclosure-phase={groupMotion.phase}
          className='flowy-disclosure-content flex flex-col gap-2px'
        >
          {baseCompanions.map(renderCompanion)}
          {overflowMotion.shouldRender && overflowCompanions.length > 0 && (
            <div
              aria-hidden={overflowMotion.phase === 'exiting'}
              data-disclosure-phase={overflowMotion.phase}
              className='flowy-disclosure-content flex flex-col'
            >
              {overflowCompanions.map(renderCompanion)}
            </div>
          )}
          {visibleCompanions.hasOverflow && !forceShowActiveCompanion && (
            <SessionOverflowButton
              expanded={showAllCompanions}
              hiddenCount={companionOverflowCount}
              controlsId='flowy-companion-sessions'
              onToggle={toggleOverflow}
              className='flowy-companion-session-overflow'
            />
          )}
        </div>
      )}
    </div>
  );
};

export default CompanionSessionGroup;
