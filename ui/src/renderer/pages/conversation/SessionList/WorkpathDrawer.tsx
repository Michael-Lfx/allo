

import { Checkbox, Dropdown, Menu, Message, Popover, Tooltip } from '@arco-design/web-react';
import {
  BookOne,
  BranchOne,
  Copy,
  DeleteOne,
  FolderClose,
  FolderOpen,
  Home,
  MoreOne,
  Plus,
  Pushpin,
} from '@icon-park/react';
import classNames from 'classnames';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import CapabilityIcon, { CAPABILITY_COLORS } from '@/renderer/components/capability/CapabilityIcon';
import MarqueeText from '@/renderer/components/base/MarqueeText';
import PathText from '@/renderer/components/base/PathText';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import WorkpathHoverCard from '@/renderer/pages/conversation/components/WorkpathHoverCard';
import { copyText } from '@/renderer/utils/ui/clipboard';
import type { ConversationId } from '@/common/types/ids';

import type { WorkpathUiState } from './hooks/useWorkpathUiState';
import { useDisclosureMotion } from './hooks/useDisclosureMotion';
import { useWorkpathKnowledgeLit } from './hooks/useWorkpathKnowledge';
import SessionOverflowButton from './SessionOverflowButton';
import {
  getBatchSelectionScopeState,
  getWorkpathBatchSelectionScope,
  type BatchSelectableScope,
  type BatchSelectionState,
} from './utils/batchSelectionScopes';
import { DEFAULT_WORKPATH_KEY, workpathKey } from './utils/workpathKey';
import {
  getVisibleWorkpathEntries,
  getWorkpathEntryDisplayIndex,
  WORKPATH_COLLAPSED_SESSION_LIMIT,
} from './utils/workpathVisibleEntries';
import { getRenderedExpansionState } from './utils/workpathExpansion';
import {
  DEFAULT_SIDEBAR_DISPLAY_PREFERENCES,
  formatWorkpathDisplay,
  type SidebarDisplayPreferences,
} from './utils/sidebarDisplayPreferences';
import { getWorkpathMenuActionKeys } from './utils/workpathMenuActions';
import type { SessionEntry, WorkpathNode } from './utils/workpathTree';

export interface WorkpathDrawerProps {
  node: WorkpathNode;
  ui: WorkpathUiState;
  /**
   * The interactive conversation currently open via `/conversation/:id`. When
   * it belongs to this node, the drawer is forced open — visually only, never
   * written back to localStorage.
   */
  activeConversationId: ConversationId | null;
  onCreateInteractive: (node: WorkpathNode) => void;
  onRemoveProjectWorkpath?: (node: WorkpathNode) => void;
  isProjectWorkpath?: boolean;
  batchMode?: boolean;
  batchSelectionState?: BatchSelectionState;
  onToggleBatchSelectionScope?: (scope: BatchSelectableScope) => void;
  renderEntry: (entry: SessionEntry) => React.ReactNode;
  displayPreferences?: SidebarDisplayPreferences;
  gitBranch?: string | null;
}

/**
 * First-level workpath drawer: header row (folder/home icon + display name +
 * conversation-count badge + hover ops) and, when expanded,
 * directly renders its interactive conversation rows.
 * Collapse interaction follows the WorkspaceCollapse paradigm (conditional
 * render, h-34px header, hover bg, trailing ops revealed on hover).
 */
const WorkpathDrawer: React.FC<WorkpathDrawerProps> = ({
  node,
  ui,
  activeConversationId,
  onCreateInteractive,
  onRemoveProjectWorkpath,
  isProjectWorkpath = false,
  batchMode = false,
  batchSelectionState,
  onToggleBatchSelectionScope,
  renderEntry,
  displayPreferences = DEFAULT_SIDEBAR_DISPLAY_PREFERENCES,
  gitBranch,
}) => {
  const { t } = useTranslation();
  const syncedActiveDrawerRouteRef = useRef<string | null>(null);
  const [showAllConversations, setShowAllConversations] = useState(false);
  const [drawerToggleKey, setDrawerToggleKey] = useState(0);
  const [overflowToggleKey, setOverflowToggleKey] = useState(0);
  const [workpathIdentityHovered, setWorkpathIdentityHovered] = useState(false);
  const isMobile = useLayoutContext()?.isMobile ?? false;

  const isDefault = node.key === DEFAULT_WORKPATH_KEY;
  const controlsId = `flowy-workpath-${workpathKey(node.key).replace(/[^a-zA-Z0-9_-]/g, '_')}-sessions`;
  const displayName = isDefault ? t('sessionList.defaultWorkpath') : node.displayName;
  const workpathDisplay = isDefault ? null : formatWorkpathDisplay(node.key, node.displayName, displayPreferences.workpathNameMode);
  const twoLineWorkpath = workpathDisplay?.kind === 'twoLine';
  const sessionCount = node.interactive.length;
  const workpathMenuActionKeys = getWorkpathMenuActionKeys({
    isDefault,
    isProjectWorkpath,
    canRemoveProjectWorkpath: Boolean(onRemoveProjectWorkpath),
  });

  const activeEntry =
    activeConversationId === null ? null : (node.interactive.find((entry) => entry.id === activeConversationId) ?? null);
  const activeDisplayIndex = activeEntry ? getWorkpathEntryDisplayIndex(node, activeEntry) : null;
  const forceShowAllForActiveConversation = activeDisplayIndex !== null && activeDisplayIndex >= WORKPATH_COLLAPSED_SESSION_LIMIT;
  const visibleEntries = getVisibleWorkpathEntries(node, {
    interactive: showAllConversations || forceShowAllForActiveConversation,
    terminal: false,
  });
  const hasInteractiveContent =
    node.interactive.length > 0 || visibleEntries.kindMeta.interactive.hasOverflow;
  const interactiveOverflowCount = Math.max(0, node.interactive.length - WORKPATH_COLLAPSED_SESSION_LIMIT);
  const baseInteractiveEntries = node.interactive.slice(0, WORKPATH_COLLAPSED_SESSION_LIMIT);
  const overflowInteractiveEntries = node.interactive.slice(WORKPATH_COLLAPSED_SESSION_LIMIT);
  const activeRouteKey = activeEntry ? `${node.key}:${activeEntry.id}` : null;
  const drawerExpansion = getRenderedExpansionState({
    active: activeEntry !== null,
    persistedExpanded: ui.isExpanded(node.key),
    activeRouteSynced: syncedActiveDrawerRouteRef.current === activeRouteKey,
  });
  const expanded = drawerExpansion.expanded;
  const drawerMotion = useDisclosureMotion(expanded, drawerToggleKey);
  const overflowMotion = useDisclosureMotion(
    showAllConversations || forceShowAllForActiveConversation,
    overflowToggleKey
  );

  // Workpath-level capability: knowledge base. P2 临时点亮规则（组内任一成员
  // binding enabled）— Task 11 / P3 切到 workpath 级单次查询后由 hook 内部替换。
  const knowledgeLit = useWorkpathKnowledgeLit(node, expanded);
  const selectedState: BatchSelectionState = batchSelectionState ?? {
    conversationIds: new Set<ConversationId>(),
    terminalIds: new Set(),
  };
  const workpathSelectionScope = getWorkpathBatchSelectionScope(node, 'interactive');
  const workpathSelectionState = getBatchSelectionScopeState(workpathSelectionScope, selectedState);

  useEffect(() => {
    if (!activeRouteKey) return;
    if (drawerExpansion.shouldSyncExpanded) {
      syncedActiveDrawerRouteRef.current = activeRouteKey;
      ui.expand(node.key);
    }
  }, [
    activeRouteKey,
    drawerExpansion.shouldSyncExpanded,
    node.key,
    ui,
  ]);

  const toggleDrawer = () => {
    setDrawerToggleKey((value) => value + 1);
    ui.toggleExpanded(node.key);
  };

  const toggleOverflow = () => {
    setOverflowToggleKey((value) => value + 1);
    setShowAllConversations((value) => !value);
  };

  const headerIcon = isDefault ? (
    <Home theme='outline' size={16} fill='currentColor' className='line-height-0' />
  ) : expanded ? (
    <FolderOpen theme='outline' size={16} fill='currentColor' className='line-height-0' />
  ) : (
    <FolderClose theme='outline' size={16} fill='currentColor' className='line-height-0' />
  );

  const nameSpan = (
    <MarqueeText
      text={displayName}
      trigger='hover'
      disabled={batchMode || isMobile}
      active={workpathIdentityHovered && !isMobile}
      className='min-w-0 flex-1 text-14px font-[500] text-t-primary'
    />
  );
  const renderWorkpathName = () => {
    if (isDefault || !workpathDisplay) return nameSpan;
    if (workpathDisplay.kind === 'compressed') {
      return (
        <span className='min-w-0 flex-1'>
          <PathText
            path={node.key}
            className='text-14px font-[500] text-t-primary'
            marqueeOnHover={!batchMode && !isMobile}
            marqueeActive={workpathIdentityHovered && !isMobile}
          />
        </span>
      );
    }
    if (workpathDisplay.kind === 'single') {
      return (
        <MarqueeText
          text={workpathDisplay.primary}
          trigger='hover'
          disabled={batchMode || isMobile}
          active={workpathIdentityHovered && !isMobile}
          className='min-w-0 flex-1 text-14px font-[500] text-t-primary'
        />
      );
    }
    return (
      <span className='flowy-workpath-two-line min-w-0 flex-1 flex flex-col justify-center overflow-hidden gap-2px'>
        <MarqueeText
          text={workpathDisplay.primary}
          trigger='hover'
          disabled={batchMode || isMobile}
          active={workpathIdentityHovered && !isMobile}
          className='min-w-0 text-14px font-[500] text-t-primary leading-16px'
        />
        {workpathDisplay.secondary && (
          <PathText
            path={workpathDisplay.secondary}
            className='flowy-workpath-secondary text-11px font-[400] text-t-secondary leading-13px'
            marqueeOnHover={!batchMode && !isMobile}
            marqueeActive={workpathIdentityHovered && !isMobile}
          />
        )}
      </span>
    );
  };

  const branchBadge =
    displayPreferences.showGitBranch && !isDefault && gitBranch ? (
      <Tooltip content={t('sessionList.currentGitBranch', { branch: gitBranch })} position='top'>
        <span className='shrink-0 max-w-78px h-18px px-5px rd-4px bg-fill-2 text-11px text-t-secondary flex items-center gap-3px min-w-0'>
          <BranchOne theme='outline' size='11' fill='currentColor' className='shrink-0' />
          <span className='truncate min-w-0'>{gitBranch}</span>
        </span>
      </Tooltip>
    ) : null;

  return (
    <div className='workpath-drawer min-w-0'>
      {/* Drawer header */}
      <div
        data-testid='workpath-toggle-row'
        className={classNames(
          'flowy-workpath-drawer-header relative flex items-center gap-6px pl-10px pr-56px rd-6px min-w-0 group',
          twoLineWorkpath ? 'flowy-workpath-header-two-line h-42px py-4px' : 'h-34px'
        )}
      >
        {batchMode && (
          <span
            className='shrink-0 flex-center'
            onClick={(e) => {
              e.stopPropagation();
            }}
          >
            <Checkbox
              checked={workpathSelectionState.checked}
              indeterminate={workpathSelectionState.indeterminate}
              disabled={workpathSelectionState.disabled}
              className='session-batch-selection-checkbox'
              onChange={() => onToggleBatchSelectionScope?.(workpathSelectionScope)}
            />
          </span>
        )}
        {/* The hover card is read-only context, so the popup never intercepts
            pointer events on their way to the row actions, and it only opens
            from the identity zone below. */}
        <Popover
          trigger='hover'
          position='right'
          content={
            <WorkpathHoverCard
              displayName={displayName}
              conversationCount={sessionCount}
              workspacePath={isDefault ? undefined : node.key}
            />
          }
          triggerProps={{ mouseEnterDelay: 400, popupStyle: { padding: 0 } }}
          getPopupContainer={() => document.body}
          style={{ maxWidth: 'calc(100vw - 24px)', pointerEvents: 'none' }}
        >
          <button
            type='button'
            aria-expanded={batchMode ? undefined : expanded}
            aria-controls={batchMode ? undefined : controlsId}
            className='flex min-w-0 flex-1 items-center gap-8px appearance-none border-none bg-transparent p-0 text-left'
            onClick={() => {
              if (batchMode && !workpathSelectionState.disabled) {
                onToggleBatchSelectionScope?.(workpathSelectionScope);
                return;
              }
              toggleDrawer();
            }}
            onPointerEnter={() => setWorkpathIdentityHovered(true)}
            onPointerLeave={() => setWorkpathIdentityHovered(false)}
          >
            <span
              className='relative size-22px flex items-center justify-center shrink-0 text-t-primary'
            >
              {headerIcon}
              {node.pinned && (
                <span
                  data-testid='workpath-pinned-badge'
                  role='img'
                  aria-label={t('sessionList.pinnedWorkpath')}
                  className='absolute -top-4px -right-5px z-1 flex-center text-[rgb(var(--primary-6))] pointer-events-none'
                >
                  <Pushpin theme='filled' size='10' fill='currentColor' className='block' />
                </span>
              )}
            </span>

            <div className='flex-1 min-w-0 flex items-center gap-6px overflow-hidden'>
              {/* Workpath capability markers live with the identity text, not the
                  hover action slot, so they never disappear under create/pin ops. */}
              {knowledgeLit && (
                <span className='shrink-0 flex items-center'>
                  <CapabilityIcon
                    icon={<BookOne theme='outline' size={13} fill='currentColor' />}
                    color={CAPABILITY_COLORS.primary}
                    title={t('knowledge.title')}
                    size={13}
                  />
                </span>
              )}

              {/* Default node shows its localized label; real workpaths follow the
                  user's display preference, with the complete path still available
                  from the tooltip, hover card, and copy op beside it. */}
              {renderWorkpathName()}
              {branchBadge}
            </div>
          </button>
        </Popover>

        {/* Hover ops: keep the high-frequency create action visible and move
            lower-frequency workpath actions behind the same more-menu pattern
            used by conversation rows. */}
        {!batchMode && (
          <span
            className='absolute right-8px top-1/2 flex -translate-y-1/2 shrink-0 items-center gap-4px opacity-0 pointer-events-none transition-opacity group-hover:opacity-100 group-hover:pointer-events-auto group-focus-within:opacity-100 group-focus-within:pointer-events-auto'
            onClick={(e) => e.stopPropagation()}
          >
            <span
              data-testid='workpath-create-interactive-btn'
              role='button'
              tabIndex={0}
              aria-label={t('sessionList.newInteractive')}
              className='flex-center cursor-pointer transition-colors text-t-tertiary hover:text-t-primary size-18px rd-4px sider-action-btn workpath-action-btn'
              onClick={(e) => {
                e.stopPropagation();
                onCreateInteractive(node);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  e.stopPropagation();
                  onCreateInteractive(node);
                }
              }}
            >
              <Plus theme='outline' size='14' fill='currentColor' className='block leading-none' />
            </span>
            <Dropdown
              droplist={
                <Menu
                  onClickMenuItem={(key) => {
                    if (key === 'copy') {
                      copyText(node.key)
                        .then(() => Message.success(t('common.copySuccess')))
                        .catch(() => Message.error(t('common.copyFailed')));
                      return;
                    }
                    if (key === 'pin') {
                      ui.togglePinned(node.key);
                      return;
                    }
                    if (key === 'remove' && onRemoveProjectWorkpath) {
                      onRemoveProjectWorkpath(node);
                    }
                  }}
                >
                  {workpathMenuActionKeys.includes('copy') && (
                    <Menu.Item key='copy'>
                      <div className='flex items-center gap-8px'>
                        <Copy theme='outline' size='14' />
                        <span>{t('common.copyPath')}</span>
                      </div>
                    </Menu.Item>
                  )}
                  {workpathMenuActionKeys.includes('pin') && (
                    <Menu.Item key='pin'>
                      <div className='flex items-center gap-8px'>
                        <Pushpin
                          theme='outline'
                          size='14'
                          className={node.pinned ? 'text-aou-7' : 'text-t-secondary'}
                        />
                        <span>{node.pinned ? t('sessionList.unpinWorkpath') : t('sessionList.pinWorkpath')}</span>
                      </div>
                    </Menu.Item>
                  )}
                  {workpathMenuActionKeys.includes('remove') && onRemoveProjectWorkpath && (
                    <Menu.Item key='remove'>
                      <div className='flex items-center gap-8px text-[rgb(var(--warning-6))]'>
                        <DeleteOne theme='outline' size='14' />
                        <span>{t('sessionList.removeWorkpath')}</span>
                      </div>
                    </Menu.Item>
                  )}
                </Menu>
              }
              trigger='click'
              position='br'
              getPopupContainer={() => document.body}
              unmountOnExit={false}
            >
              <span
                data-testid='workpath-more-actions-btn'
                role='button'
                tabIndex={0}
                aria-label={t('common.more')}
                className='flex-center cursor-pointer transition-colors text-t-tertiary hover:text-t-primary size-20px rd-4px sider-action-btn workpath-action-btn'
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    e.stopPropagation();
                    e.currentTarget.click();
                  }
                }}
              >
                <MoreOne theme='outline' size='14' fill='currentColor' className='block leading-none' />
              </span>
            </Dropdown>
          </span>
        )}
      </div>

      {/* Drawer content: workpaths expose interactive conversations directly. */}
      {drawerMotion.shouldRender && (
        <div
          id={controlsId}
          data-testid='workpath-conversation-list'
          aria-hidden={drawerMotion.phase === 'exiting'}
          data-disclosure-phase={drawerMotion.phase}
          className={classNames(
            'workpath-drawer-content flowy-disclosure-content min-w-0 flex flex-col',
            'flowy-workpath-session-list',
            hasInteractiveContent && 'gap-2px pt-2px'
          )}
        >
          {baseInteractiveEntries.map((entry) => renderEntry(entry))}
          {overflowMotion.shouldRender && overflowInteractiveEntries.length > 0 && (
            <div
              aria-hidden={overflowMotion.phase === 'exiting'}
              data-disclosure-phase={overflowMotion.phase}
              className='flowy-disclosure-content flex flex-col'
            >
              {overflowInteractiveEntries.map((entry) => renderEntry(entry))}
            </div>
          )}
          {visibleEntries.kindMeta.interactive.hasOverflow && !forceShowAllForActiveConversation && (
            <SessionOverflowButton
              expanded={showAllConversations}
              hiddenCount={interactiveOverflowCount}
              controlsId={controlsId}
              onToggle={toggleOverflow}
              className='flowy-workpath-session-overflow'
            />
          )}
        </div>
      )}
    </div>
  );
};

export default WorkpathDrawer;
