

import { Checkbox, Popover, Tooltip } from '@arco-design/web-react';
import { BookOne, BranchOne, DeleteOne, FolderClose, FolderOpen, Home, Plus, Pushpin } from '@icon-park/react';
import classNames from 'classnames';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import CapabilityIcon, { CAPABILITY_COLORS } from '@/renderer/components/capability/CapabilityIcon';
import CopyIconButton from '@/renderer/components/base/CopyIconButton';
import PathText from '@/renderer/components/base/PathText';
import WorkpathHoverCard from '@/renderer/pages/conversation/components/WorkpathHoverCard';
import type { ConversationId } from '@/common/types/ids';

import type { WorkpathUiState } from './hooks/useWorkpathUiState';
import { useWorkpathKnowledgeLit } from './hooks/useWorkpathKnowledge';
import {
  getBatchSelectionScopeState,
  getWorkpathBatchSelectionScope,
  type BatchSelectableScope,
  type BatchSelectionState,
} from './utils/batchSelectionScopes';
import { DEFAULT_WORKPATH_KEY } from './utils/workpathKey';
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
 * First-level workpath drawer: header row (expand arrow + folder/home icon +
 * display name + conversation-count badge + hover ops) and, when expanded,
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

  const isDefault = node.key === DEFAULT_WORKPATH_KEY;
  const displayName = isDefault ? t('sessionList.defaultWorkpath') : node.displayName;
  const workpathDisplay = isDefault ? null : formatWorkpathDisplay(node.key, node.displayName, displayPreferences.workpathNameMode);
  const twoLineWorkpath = workpathDisplay?.kind === 'twoLine';
  const sessionCount = node.interactive.length;

  const activeEntry =
    activeConversationId === null ? null : (node.interactive.find((entry) => entry.id === activeConversationId) ?? null);
  const activeDisplayIndex = activeEntry ? getWorkpathEntryDisplayIndex(node, activeEntry) : null;
  const forceShowAllForActiveConversation = activeDisplayIndex !== null && activeDisplayIndex >= WORKPATH_COLLAPSED_SESSION_LIMIT;
  const visibleEntries = getVisibleWorkpathEntries(node, {
    interactive: showAllConversations || forceShowAllForActiveConversation,
    terminal: false,
  });
  const hasInteractiveContent =
    visibleEntries.interactive.length > 0 || visibleEntries.kindMeta.interactive.hasOverflow;
  const activeRouteKey = activeEntry ? `${node.key}:${activeEntry.id}` : null;
  const drawerExpansion = getRenderedExpansionState({
    active: activeEntry !== null,
    persistedExpanded: ui.isExpanded(node.key),
    activeRouteSynced: syncedActiveDrawerRouteRef.current === activeRouteKey,
  });
  const expanded = drawerExpansion.expanded;

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
    ui.toggleExpanded(node.key);
  };

  const headerIcon = isDefault ? (
    <Home theme='outline' size={16} fill='currentColor' className='line-height-0' />
  ) : expanded ? (
    <FolderOpen theme='outline' size={16} fill='currentColor' className='line-height-0' />
  ) : (
    <FolderClose theme='outline' size={16} fill='currentColor' className='line-height-0' />
  );

  const nameSpan = (
    <span className='text-14px font-[500] truncate text-t-primary min-w-0'>{displayName}</span>
  );
  const renderWorkpathName = () => {
    if (isDefault || !workpathDisplay) return nameSpan;
    if (workpathDisplay.kind === 'compressed') {
      return (
        <span className='inline-flex min-w-0'>
          <PathText path={node.key} className='text-14px font-[500] text-t-primary' />
        </span>
      );
    }
    if (workpathDisplay.kind === 'single') {
      return (
        <span className='text-14px font-[500] truncate text-t-primary min-w-0'>{workpathDisplay.primary}</span>
      );
    }
    return (
      <span className='min-w-0 flex-1 flex flex-col justify-center overflow-hidden gap-2px'>
        <span className='text-13px font-[500] truncate text-t-primary leading-16px'>{workpathDisplay.primary}</span>
        {workpathDisplay.secondary && (
          <PathText path={workpathDisplay.secondary} className='text-11px font-[400] text-t-secondary leading-13px' />
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
        style={{ maxWidth: 'calc(100vw - 24px)' }}
      >
        <div
          className={classNames(
            'flex items-center gap-8px pl-10px pr-8px cursor-pointer hover:bg-fill-2 rd-10px transition-colors min-w-0 group',
            twoLineWorkpath ? 'h-42px py-4px' : 'h-34px'
          )}
          onClick={() => {
            if (batchMode && !workpathSelectionState.disabled) {
              onToggleBatchSelectionScope?.(workpathSelectionScope);
              return;
            }
            toggleDrawer();
          }}
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
          <span
            className='size-22px flex items-center justify-center shrink-0 text-t-primary'
            onClick={(e) => {
              e.stopPropagation();
              toggleDrawer();
            }}
          >
            {headerIcon}
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

          {/* Pinned dot indicator (rest state; hidden once hover ops appear) */}
          {!batchMode && node.pinned && <span className='size-6px rd-full shrink-0 bg-aou-1 group-hover:hidden' />}

          {/* Hover ops: copy path + direct interactive-session create + pin toggle. */}
          {!batchMode && (
            <span
              className='hidden group-hover:flex shrink-0 items-center gap-6px'
              onClick={(e) => e.stopPropagation()}
            >
              {!isDefault && (
                <CopyIconButton
                  text={node.key}
                  tooltip={t('common.copyPath')}
                  className='shrink-0 size-18px sider-action-btn workpath-action-btn text-t-tertiary'
                />
              )}
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
              <Tooltip content={node.pinned ? t('sessionList.unpinWorkpath') : t('sessionList.pinWorkpath')} position='top'>
                <span
                  role='button'
                  tabIndex={0}
                  aria-label={node.pinned ? t('sessionList.unpinWorkpath') : t('sessionList.pinWorkpath')}
                  className={classNames(
                    'flex-center cursor-pointer transition-colors hover:text-t-primary size-18px rd-4px sider-action-btn workpath-action-btn',
                    node.pinned ? 'text-aou-1' : 'text-t-tertiary'
                  )}
                  onClick={(e) => {
                    e.stopPropagation();
                    ui.togglePinned(node.key);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      e.stopPropagation();
                      ui.togglePinned(node.key);
                    }
                  }}
                >
                  <Pushpin theme='outline' size='14' fill='currentColor' className='block leading-none' />
                </span>
              </Tooltip>
              {!isDefault && isProjectWorkpath && onRemoveProjectWorkpath && (
                <Tooltip content={t('sessionList.removeWorkpath')} position='top'>
                  <span
                    role='button'
                    tabIndex={0}
                    aria-label={t('sessionList.removeWorkpath')}
                    className='flex-center cursor-pointer transition-colors text-t-tertiary hover:text-t-primary size-20px rd-4px sider-action-btn workpath-action-btn'
                    onClick={(e) => {
                      e.stopPropagation();
                      onRemoveProjectWorkpath(node);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        e.stopPropagation();
                        onRemoveProjectWorkpath(node);
                      }
                    }}
                  >
                    <DeleteOne theme='outline' size='14' fill='currentColor' className='block leading-none' />
                  </span>
                </Tooltip>
              )}
            </span>
          )}
        </div>
      </Popover>

      {/* Drawer content: workpaths expose interactive conversations directly. */}
      {expanded && (
        <div
          data-testid='workpath-conversation-list'
          className={classNames(
            'workpath-drawer-content min-w-0 flex flex-col',
            hasInteractiveContent && 'gap-2px pt-2px'
          )}
        >
          {visibleEntries.interactive.map((entry) => renderEntry(entry))}
          {visibleEntries.kindMeta.interactive.hasOverflow && !forceShowAllForActiveConversation && (
            <button
              type='button'
              aria-expanded={showAllConversations}
              className='ml-42px mt-1px mb-2px inline-flex h-20px w-fit max-w-full appearance-none items-center border-none bg-transparent p-0 text-left text-12px leading-20px text-t-secondary transition-colors cursor-pointer select-none hover:text-t-primary focus:outline-none focus-visible:text-t-primary'
              onClick={(event) => {
                event.stopPropagation();
                setShowAllConversations((value) => !value);
              }}
            >
              {showAllConversations
                ? t('sessionList.collapseDisplay')
                : t('sessionList.expandDisplay', { count: visibleEntries.kindMeta.interactive.hiddenCount })}
            </button>
          )}
        </div>
      )}
    </div>
  );
};

export default WorkpathDrawer;
