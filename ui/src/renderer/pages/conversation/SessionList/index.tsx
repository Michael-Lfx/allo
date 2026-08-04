

import { ipcBridge } from '@/common';
import type { TChatConversation } from '@/common/config/storage';
import type { ConversationId } from '@/common/types/ids';
import BatchActionBar from '@/renderer/components/base/BatchActionBar';
import DirectorySelectionModal from '@/renderer/components/settings/DirectorySelectionModal';
import { getRecentWorkspaces } from '@/renderer/components/workspace/recentWorkspaces';
import { useConversationHistoryContext } from '@/renderer/hooks/context/ConversationHistoryContext';
import { useCronJobsMap } from '@/renderer/pages/cron';
import { emitter } from '@/renderer/utils/emitter';
import { parseSessionRoute } from '@/renderer/utils/routes/sessionRoute';
import { scrollSidebarItemIntoView } from '@/renderer/utils/ui/scrollIntoView';
import { cleanupSiderTooltips } from '@/renderer/utils/ui/siderTooltip';
import { Input, Message, Modal } from '@arco-design/web-react';
import { FolderOpen, Plus, Right } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import ConversationRow from './ConversationRow';
import CompanionSessionGroup from './CompanionSessionGroup';
import { useBatchSelection } from './hooks/useBatchSelection';
import { useConversationActions } from './hooks/useConversationActions';
import { useExport } from './hooks/useExport';
import { capabilityKey, useSessionCapabilities } from './hooks/useSessionCapabilities';
import { useWorkpathBranches } from './hooks/useWorkpathBranches';
import type { ConversationRowProps } from './types';
import WorkpathDrawer from './WorkpathDrawer';
import { useWorkpathUiState } from './hooks/useWorkpathUiState';
import { toggleBatchSelectionScope, type BatchSelectableScope, type BatchSelectionState } from './utils/batchSelectionScopes';
import { DEFAULT_WORKPATH_KEY } from './utils/workpathKey';
import { buildWorkpathTree } from './utils/workpathTree';
import {
  getProjectWorkpaths,
  removeProjectWorkpath,
  subscribeProjectWorkpaths,
} from './utils/projectWorkpaths';
import {
  DEFAULT_SIDEBAR_DISPLAY_PREFERENCES,
  type SidebarDisplayPreferences,
} from './utils/sidebarDisplayPreferences';
import type { SessionEntry, WorkpathNode } from './utils/workpathTree';

export type WorkpathSessionListProps = {
  onSessionClick?: () => void;
  collapsed?: boolean;
  tooltipEnabled?: boolean;
  batchMode?: boolean;
  displayPreferences?: SidebarDisplayPreferences;
  onBatchModeChange?: (value: boolean) => void;
};

/**
 * Workpath session list: each workpath directly contains its interactive
 * conversations. Terminal sessions remain available through their dedicated
 * routes and entry points, outside this conversation hierarchy.
 */
const WorkpathSessionList: React.FC<WorkpathSessionListProps> = ({
  onSessionClick,
  collapsed = false,
  tooltipEnabled = false,
  batchMode = false,
  displayPreferences = DEFAULT_SIDEBAR_DISPLAY_PREFERENCES,
  onBatchModeChange,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  // 控制工作路径列表的展开/收起
  const [expanded, setExpanded] = useState(true);
  // 悬浮菜单位置和状态
  const [dropdownStyle, setDropdownStyle] = useState<React.CSSProperties>({});
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownTriggerRef = useRef<HTMLButtonElement | null>(null);
  const dropdownRef = useRef<HTMLDivElement | null>(null);
  // 每次打开悬浮菜单时重新读取 localStorage（addRecentWorkspace 写入后需要刷新快照）
  const [recentWorkspaces, setRecentWorkspaces] = useState<string[]>(() => getRecentWorkspaces());
  const {
    getJobStatus,
    markAsRead,
    setActiveConversation: setCronActiveConversation,
  } = useCronJobsMap();
  // AutoWork / IDMM enabled-state snapshot (bulk fetch + WS events, no per-row requests).
  const capabilities = useSessionCapabilities();

  const {
    conversations,
    isConversationGenerating,
    hasCompletionUnread,
    clearCompletionUnread,
    setActiveConversation,
  } = useConversationHistoryContext();
  const ui = useWorkpathUiState();
  const [emptyProjectWorkpaths, setEmptyProjectWorkpaths] = useState<string[]>(() => getProjectWorkpaths());

  useEffect(() => {
    return subscribeProjectWorkpaths(() => setEmptyProjectWorkpaths(getProjectWorkpaths()));
  }, []);

  const tree = useMemo(
    () => buildWorkpathTree(conversations, [], ui.pinnedKeys, emptyProjectWorkpaths),
    [conversations, ui.pinnedKeys, emptyProjectWorkpaths]
  );

  const projectWorkpathKeys = useMemo(() => new Set(emptyProjectWorkpaths), [emptyProjectWorkpaths]);
  const branchWorkpaths = useMemo(
    () => tree.filter((node) => node.key !== DEFAULT_WORKPATH_KEY).map((node) => node.key),
    [tree]
  );
  const workpathBranches = useWorkpathBranches(branchWorkpaths, displayPreferences.showGitBranch && !collapsed);

  // Active session from the route — used for row selected state and drawer expansion.
  const activeRoute = useMemo(() => parseSessionRoute(pathname), [pathname]);
  const activeConversationId = activeRoute?.kind === 'conversation' ? activeRoute.id : null;

  // Sync active-conversation bookkeeping + scroll it into view on route change
  // (carried over from GroupedHistory / useConversations).
  useEffect(() => {
    if (!activeConversationId) {
      setActiveConversation(null);
      return;
    }
    setActiveConversation(activeConversationId);
    setCronActiveConversation(activeConversationId);
    clearCompletionUnread(activeConversationId);
    return scrollSidebarItemIntoView('c-' + activeConversationId);
  }, [activeConversationId, setActiveConversation, setCronActiveConversation, clearCompletionUnread]);

  /* ------------------------------- batch selection ------------------------------- */

  // All interactive sessions are selectable (the old "project members excluded"
  // rule is gone — projects no longer exist as a separate grouping).
  const {
    selectedConversationIds,
    setSelectedConversationIds,
    toggleSelectedConversation,
  } = useBatchSelection(batchMode, conversations);

  const totalSelected = selectedConversationIds.size;
  const totalSelectable = conversations.length;
  const allSelected = totalSelectable > 0 && totalSelected === totalSelectable;
  const batchSelectionState = useMemo<BatchSelectionState>(
    () => ({
      conversationIds: selectedConversationIds,
      terminalIds: new Set(),
    }),
    [selectedConversationIds]
  );
  const handleToggleSelectAll = useCallback(() => {
    if (allSelected) {
      setSelectedConversationIds(new Set());
    } else {
      setSelectedConversationIds(new Set(conversations.map((conversation) => conversation.id)));
    }
  }, [allSelected, conversations, setSelectedConversationIds]);
  const handleToggleBatchSelectionScope = useCallback(
    (scope: BatchSelectableScope) => {
      const next = toggleBatchSelectionScope(scope, batchSelectionState);
      setSelectedConversationIds(next.conversationIds);
    },
    [batchSelectionState, setSelectedConversationIds]
  );

  /* --------------------------- per-row actions & modals --------------------------- */

  const {
    renameModalVisible,
    renameModalName,
    setRenameModalName,
    renameLoading,
    dropdownVisibleId,
    handleConversationClick,
    handleDeleteClick,
    handleEditStart,
    handleRenameConfirm,
    handleRenameCancel,
    handleTogglePin,
    handleMenuVisibleChange,
    handleOpenMenu,
  } = useConversationActions({
    activeConversationId,
    batchMode,
    onSessionClick,
    onBatchModeChange,
    selectedConversationIds,
    setSelectedConversationIds,
    toggleSelectedConversation,
    markAsRead,
  });

  const {
    exportTask,
    exportModalVisible,
    exportTargetPath,
    exportModalLoading,
    showExportDirectorySelector,
    setShowExportDirectorySelector,
    closeExportModal,
    handleSelectExportDirectoryFromModal,
    handleSelectExportFolder,
    handleExportConversation,
    handleBatchExport,
    handleConfirmExport,
  } = useExport({
    conversations,
    selectedConversationIds,
    setSelectedConversationIds,
    onBatchModeChange,
  });

  // Batch deletion is scoped to the interactive conversations rendered here.
  const handleBatchDeleteAll = useCallback(() => {
    const convIds = Array.from(selectedConversationIds);
    const total = convIds.length;
    if (total === 0) {
      Message.warning(t('conversation.history.batchNoSelection'));
      return;
    }
    Modal.confirm({
      title: t('conversation.history.batchDelete', { count: total }),
      content: t('conversation.history.batchDeleteConfirm', { count: total }),
      okText: t('conversation.history.confirmDelete'),
      cancelText: t('conversation.history.cancelDelete'),
      okButtonProps: { status: 'warning' },
      onOk: async () => {
        let successCount = 0;
        try {
          const convResults = await Promise.all(
            convIds.map(async (conversation_id) => {
              try {
                await ipcBridge.conversation.remove.invoke({ conversation_id: conversation_id });
                emitter.emit('conversation.deleted', conversation_id);
                if (activeConversationId === conversation_id) void navigate('/guid');
                return true;
              } catch {
                return false;
              }
            })
          );
          successCount += convResults.filter(Boolean).length;
          emitter.emit('chat.history.refresh');
          if (successCount > 0) {
            Message.success(t('conversation.history.batchDeleteSuccess', { count: successCount }));
          } else {
            Message.error(t('conversation.history.deleteFailed'));
          }
        } finally {
          setSelectedConversationIds(new Set());
          onBatchModeChange?.(false);
        }
      },
      style: { borderRadius: '12px' },
      alignCenter: true,
      getPopupContainer: () => document.body,
    });
  }, [
    selectedConversationIds,
    activeConversationId,
    navigate,
    onBatchModeChange,
    setSelectedConversationIds,
    t,
  ]);

  /* ----------------------------- create-session entries ----------------------------- */

  const handleCreateInteractive = useCallback(
    (node: WorkpathNode) => {
      // default 节点不带 state —— 走普通「新建对话」流程
      if (node.key === DEFAULT_WORKPATH_KEY) {
        void navigate('/guid');
      } else {
        void navigate('/guid', { state: { workspace: node.key } });
      }
      onSessionClick?.();
    },
    [navigate, onSessionClick]
  );

  const handleRemoveProjectWorkpath = useCallback(
    (node: WorkpathNode) => {
      if (node.key === DEFAULT_WORKPATH_KEY) return;
      const sessionCount = node.interactive.length;
      Modal.confirm({
        title: t('sessionList.removeWorkpathTitle'),
        content: t(
          sessionCount > 0
            ? 'sessionList.removeWorkpathWithSessionsConfirm'
            : 'sessionList.removeWorkpathConfirm',
          { path: node.key, count: sessionCount }
        ),
        okText: t('common.remove'),
        cancelText: t('common.cancel'),
        okButtonProps: { status: 'danger' },
        onOk: () => {
          removeProjectWorkpath(node.key);
          if (node.pinned) ui.togglePinned(node.key);
          setEmptyProjectWorkpaths(getProjectWorkpaths());
          Message.success(t('sessionList.removeWorkpathSuccess'));
        },
        style: { borderRadius: '12px' },
        alignCenter: true,
        getPopupContainer: () => document.body,
      });
    },
    [t, ui]
  );

  /* ------------------------------- reveal on create ------------------------------- */

  // Auto-expand the owning drawer and scroll a newly created conversation into
  // view (adapted from Sider/useRevealOnCreate: event-driven, never
  // fires on initial load; reveal waits until the row lands in the tree because
  // the workpath is only known after aggregation). Last-one-wins on bursts.
  const pendingRevealRef = useRef<ConversationId | null>(null);
  const [revealTick, setRevealTick] = useState(0);

  useEffect(() => {
    const offConversationCreated = ipcBridge.conversation.listChanged.on((event) => {
      if (event.action !== 'created') return;
      pendingRevealRef.current = event.conversation_id;
      setRevealTick((tick) => tick + 1);
    });
    // TODO(cron): cron.job-created creates a *job*, not a session — its derived
    // conversation surfaces through conversation.listChanged 'created' and is
    // covered above. Revealing the anchored session of a job bound to an
    // EXISTING conversation/terminal is deferred to the cron-integration task.
    return () => {
      offConversationCreated();
    };
  }, []);

  const { expand: expandWorkpathDrawer } = ui;
  useEffect(() => {
    const pending = pendingRevealRef.current;
    if (!pending) return;
    const node = tree.find((candidate) => candidate.interactive.some((entry) => entry.id === pending));
    if (!node) return; // not aggregated yet (async list refresh) — retry on next data change
    pendingRevealRef.current = null;
    expandWorkpathDrawer(node.key);
    scrollSidebarItemIntoView('c-' + pending);
  }, [tree, revealTick, expandWorkpathDrawer]);

  /* ------------------------- workspace dropdown UI ------------------------- */

  const handleOpenDropdown = useCallback(() => {
    const el = dropdownTriggerRef.current;
    if (!el) return;
    setRecentWorkspaces(getRecentWorkspaces());
    const rect = el.getBoundingClientRect();
    // position below the trigger, aligned to left edge
    setDropdownStyle({
      position: 'fixed' as const,
      left: rect.left,
      top: rect.bottom + 6,
      minWidth: 280,
      maxWidth: 360,
      zIndex: 99999,
    });
    setDropdownOpen(true);
  }, []);

  const handleCloseDropdown = useCallback(() => {
    setDropdownOpen(false);
  }, []);

  const toggleDropdownOpen = useCallback(() => {
    if (dropdownOpen) {
      handleCloseDropdown();
    } else {
      handleOpenDropdown();
    }
  }, [dropdownOpen, handleOpenDropdown, handleCloseDropdown]);

  // 点击外部关闭悬浮菜单 + Escape 键关闭
  useEffect(() => {
    if (!dropdownOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (
        dropdownTriggerRef.current &&
        !dropdownTriggerRef.current.contains(target) &&
        dropdownRef.current &&
        !dropdownRef.current.contains(target)
      ) {
        handleCloseDropdown();
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') handleCloseDropdown();
    };
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [dropdownOpen, handleCloseDropdown]);

  /* ---------------------------------- row render ---------------------------------- */

  const getConversationRowProps = useCallback(
    (conversation: TChatConversation): ConversationRowProps => ({
      conversation,
      isGenerating: isConversationGenerating(conversation.id),
      hasCompletionUnread: hasCompletionUnread(conversation.id),
      collapsed,
      tooltipEnabled,
      batchMode,
      checked: selectedConversationIds.has(conversation.id),
      selected: activeConversationId === conversation.id,
      menuVisible: dropdownVisibleId !== null && dropdownVisibleId === conversation.id,
      onToggleChecked: toggleSelectedConversation,
      onConversationClick: handleConversationClick,
      onOpenMenu: handleOpenMenu,
      onMenuVisibleChange: handleMenuVisibleChange,
      onEditStart: handleEditStart,
      onDelete: handleDeleteClick,
      onExport: handleExportConversation,
      onTogglePin: handleTogglePin,
      getJobStatus,
      autoworkState: capabilities.autowork.get(capabilityKey('conversation', conversation.id)),
      idmmState: capabilities.idmm.get(capabilityKey('conversation', conversation.id)),
      showSessionAge: displayPreferences.sessionMetaMode === 'age',
    }),
    [
      collapsed,
      tooltipEnabled,
      batchMode,
      isConversationGenerating,
      hasCompletionUnread,
      selectedConversationIds,
      activeConversationId,
      dropdownVisibleId,
      toggleSelectedConversation,
      handleConversationClick,
      handleOpenMenu,
      handleMenuVisibleChange,
      handleEditStart,
      handleDeleteClick,
      handleExportConversation,
      handleTogglePin,
      getJobStatus,
      capabilities,
      displayPreferences.sessionMetaMode,
    ]
  );

  const renderEntry = useCallback(
    (entry: SessionEntry): React.ReactNode => {
      if (entry.kind === 'interactive' && entry.conversation) {
        return <ConversationRow key={entry.id} {...getConversationRowProps(entry.conversation)} dimIcon />;
      }
      return null;
    },
    [getConversationRowProps]
  );

  /* ------------------------------------ render ------------------------------------ */

  const modals = (
    <>
      {/* Rename modal (carried over from GroupedHistory) */}
      <Modal
        title={t('conversation.history.renameTitle')}
        visible={renameModalVisible}
        onOk={handleRenameConfirm}
        onCancel={handleRenameCancel}
        okText={t('conversation.history.saveName')}
        cancelText={t('conversation.history.cancelEdit')}
        confirmLoading={renameLoading}
        okButtonProps={{ disabled: !renameModalName.trim() }}
        style={{ borderRadius: '12px' }}
        alignCenter
        getPopupContainer={() => document.body}
      >
        <Input
          autoFocus
          value={renameModalName}
          onChange={setRenameModalName}
          onPressEnter={handleRenameConfirm}
          placeholder={t('conversation.history.renamePlaceholder')}
          allowClear
        />
      </Modal>

      {/* Export modal (carried over from GroupedHistory) */}
      <Modal
        visible={exportModalVisible}
        title={t('conversation.history.exportDialogTitle')}
        onCancel={closeExportModal}
        footer={null}
        style={{ borderRadius: '12px' }}
        className='conversation-export-modal'
        alignCenter
        getPopupContainer={() => document.body}
      >
        <div className='py-8px'>
          <div className='text-14px mb-16px text-t-secondary'>
            {exportTask?.mode === 'batch'
              ? t('conversation.history.exportDialogBatchDescription', { count: exportTask.conversation_ids.length })
              : t('conversation.history.exportDialogSingleDescription')}
          </div>

          <div className='mb-16px p-16px rounded-12px bg-fill-1'>
            <div className='text-14px mb-8px text-t-primary'>{t('conversation.history.exportTargetFolder')}</div>
            <div
              className='flex items-center justify-between px-12px py-10px rounded-8px transition-colors'
              style={{
                backgroundColor: 'var(--color-bg-1)',
                border: '1px solid var(--color-border-2)',
                cursor: exportModalLoading ? 'not-allowed' : 'pointer',
                opacity: exportModalLoading ? 0.55 : 1,
              }}
              onClick={() => {
                void handleSelectExportFolder();
              }}
            >
              <span
                className='text-14px overflow-hidden text-ellipsis whitespace-nowrap'
                style={{ color: exportTargetPath ? 'var(--color-text-1)' : 'var(--color-text-3)' }}
              >
                {exportTargetPath || t('conversation.history.exportSelectFolder')}
              </span>
              <FolderOpen theme='outline' size='18' fill='var(--color-text-3)' />
            </div>
          </div>

          <div className='flex items-center gap-8px mb-20px text-14px text-t-secondary'>
            <span>💡</span>
            <span>{t('conversation.history.exportDialogHint')}</span>
          </div>

          <div className='flex gap-12px justify-end'>
            <button
              className='px-24px py-8px rounded-20px text-14px font-medium transition-all'
              style={{
                border: '1px solid var(--color-border-2)',
                backgroundColor: 'var(--color-fill-2)',
                color: 'var(--color-text-1)',
              }}
              onMouseEnter={(event) => {
                event.currentTarget.style.backgroundColor = 'var(--color-fill-3)';
              }}
              onMouseLeave={(event) => {
                event.currentTarget.style.backgroundColor = 'var(--color-fill-2)';
              }}
              onClick={closeExportModal}
            >
              {t('common.cancel')}
            </button>
            <button
              className='px-24px py-8px rounded-20px text-14px font-medium transition-all'
              style={{
                border: 'none',
                backgroundColor: exportModalLoading ? 'var(--color-fill-3)' : 'var(--color-text-1)',
                color: 'var(--color-bg-1)',
                cursor: exportModalLoading ? 'not-allowed' : 'pointer',
              }}
              onMouseEnter={(event) => {
                if (!exportModalLoading) {
                  event.currentTarget.style.opacity = '0.85';
                }
              }}
              onMouseLeave={(event) => {
                if (!exportModalLoading) {
                  event.currentTarget.style.opacity = '1';
                }
              }}
              onClick={() => {
                void handleConfirmExport();
              }}
              disabled={exportModalLoading}
            >
              {exportModalLoading ? t('conversation.history.exporting') : t('common.confirm')}
            </button>
          </div>
        </div>
      </Modal>

      <DirectorySelectionModal
        visible={showExportDirectorySelector}
        onConfirm={handleSelectExportDirectoryFromModal}
        onCancel={() => setShowExportDirectorySelector(false)}
      />

      {/* Workspace dropdown */}
      {dropdownOpen && (
        <div
          ref={dropdownRef}
          style={{
            ...dropdownStyle,
            background: 'var(--color-bg-1, #fff)',
            border: '1px solid var(--color-border-2)',
            borderRadius: '12px',
            padding: '6px',
            boxShadow: '0 4px 12px rgba(0,0,0,0.08), 0 12px 32px rgba(0,0,0,0.06)',
            userSelect: 'none',
          }}>
          {/* Open Folder button */}
          <button
            type='button'
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
              width: '100%',
              padding: '8px 10px',
              border: 'none',
              borderRadius: '10px',
              background: 'transparent',
              cursor: 'pointer',
              fontSize: '13px',
              fontFamily: 'inherit',
              textAlign: 'left',
              color: 'var(--color-text-1)',
            }}
            onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-fill-2)'; }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; }}
            onClick={() => {
              ipcBridge.dialog.showOpen
                .invoke({ properties: ['openDirectory', 'createDirectory'] })
                .then((paths) => {
                  const projectPath = paths?.[0]?.trim();
                  if (!projectPath) return;
                  import('@/renderer/pages/conversation/SessionList/utils/projectWorkpaths').then(({ addProjectWorkpath }) => {
                    import('@/renderer/components/workspace').then(({ addRecentWorkspace }) => {
                      addProjectWorkpath(projectPath);
                      addRecentWorkspace(projectPath);
                      void navigate('/guid', { state: { workspace: projectPath } });
                      Message.success(t('sessionList.createProjectSuccess'));
                    });
                  });
                })
                .catch((error) => {
                  console.error('[Workspace] Failed to open directory:', error);
                });
            }}>
            <svg width='14' height='14' fill='none' stroke='currentColor' strokeWidth='1.8' viewBox='0 0 24 24' style={{ flexShrink: 0, color: 'var(--color-text-3)' }}>
              <path d='M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z' />
            </svg>
            <span>{t('common.filePicker.chooseDifferentFolder')}</span>
          </button>

          {/* Recents section */}
          {recentWorkspaces.length > 0 && (
            <>
              <div style={{ height: '1px', background: 'var(--color-border-2)', margin: '4px', opacity: 0.6 }} />
              <div style={{ padding: '6px 10px', fontSize: '11px', fontWeight: 600, letterSpacing: '0.5px', textTransform: 'uppercase', color: 'var(--color-text-3)' }}>
                {t('guid.workspace.recentWorkspaces') || '最近使用'}
              </div>
              <div style={{ maxHeight: '200px', overflowY: 'auto' }}>
                {recentWorkspaces.slice(0, 5).map((path) => {
                  const name = path.split(/[\\/]/).pop() || path;
                  return (
                    <button
                      type='button'
                      key={path}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: '8px',
                        width: '100%',
                        padding: '8px 10px',
                        border: 'none',
                        borderRadius: '10px',
                        background: 'transparent',
                        cursor: 'pointer',
                        fontSize: '13px',
                        fontFamily: 'inherit',
                        textAlign: 'left',
                        color: 'var(--color-text-1)',
                      }}
                      onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-fill-2)'; }}
                      onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; }}
                      onClick={() => {
                        import('@/renderer/pages/conversation/SessionList/utils/projectWorkpaths').then(({ addProjectWorkpath }) => {
                          import('@/renderer/components/workspace').then(({ addRecentWorkspace }) => {
                            addProjectWorkpath(path);
                            addRecentWorkspace(path);
                            void navigate('/guid', { state: { workspace: path } });
                            handleCloseDropdown();
                          });
                        });
                      }}>
                      <svg width='13' height='13' fill='none' stroke='currentColor' strokeWidth='1.8' viewBox='0 0 24 24' style={{ flexShrink: 0, color: 'var(--color-text-3)' }}>
                        <path d='M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7z' />
                      </svg>
                      <div style={{ minWidth: 0, flex: 1, display: 'flex', alignItems: 'baseline', gap: '6px', whiteSpace: 'nowrap', overflow: 'hidden' }}>
                        <span style={{ fontSize: '13px', lineHeight: '18px', fontWeight: 500, flexShrink: 0 }}>{name}</span>
                        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: '12px', lineHeight: '18px', color: 'var(--color-text-3)' }}>{path}</span>
                      </div>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </div>
      )}
    </>
  );

  // Collapsed sider: flat icon-only interactive conversation rows.
  if (collapsed) {
    return (
      <>
        {modals}
        <div className='min-w-0'>
          <CompanionSessionGroup
            collapsed
            activeConversationId={activeConversationId}
            onSessionClick={onSessionClick}
          />
          {tree.flatMap((node) =>
            node.interactive.map((entry) =>
              entry.conversation ? (
                <ConversationRow key={entry.id} {...getConversationRowProps(entry.conversation)} />
              ) : null
            )
          )}
        </div>
      </>
    );
  }

  return (
    <>
      {modals}
      <div className='min-w-0 mt-10px'>
        {/* 桌面伙伴专属工作空间分组（roster-driven，置于项目/工作路径之上）。仅交互式、
            不在此新建；可折叠（状态持久化于 useWorkpathUiState，默认展开）；
            点击伙伴行跳转其唯一会话 /conversation/:id。 */}
        <CompanionSessionGroup
          activeConversationId={activeConversationId}
          onSessionClick={onSessionClick}
          expanded={ui.companionGroupExpanded}
          onToggleExpanded={ui.toggleCompanionGroup}
        />

        <div data-testid='workpath-section-toolbar' className='pl-10px pr-4px pb-6px flex items-center justify-between'>
          {/* Left text — click to fold/unfold the workpath list */}
          <button
            type='button'
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
            className='group flex items-center gap-2px min-w-0 select-none cursor-pointer b-none bg-transparent p-0 text-left opacity-75 transition-opacity hover:opacity-100 relative'
          >
            <span className='sider-section-title text-13px font-[500] leading-none tracking-wide truncate min-w-0'>
              {t('sessionList.workspaces')}
            </span>
            {/* Arrow icon - shown on hover of the title button, rotates based on expanded state */}
            {(!collapsed || dropdownOpen) && (
              <div
                className={`ml-1 shrink-0 opacity-0 group-hover:opacity-100 transition-all duration-200 ${
                  expanded ? 'rotate-90' : ''
                } collapsed-hidden`}
                style={{
                  transformOrigin: 'center center',
                }}
              >
                <Right
                  theme='outline'
                  size='12'
                  fill='currentColor'
                  className='block leading-none'
                />
              </div>
            )}
          </button>
          {/* Right plus button */}
          <button
            ref={dropdownTriggerRef}
            type='button'
            onClick={toggleDropdownOpen}
            aria-label={t('common.add') || '添加'}
            style={{
              border: 'none',
              outline: 'none',
              background: 'transparent',
              padding: 0,
              margin: 0,
            }}
            className={`h-22px px-8px flex items-center gap-4px select-none cursor-pointer rd-6px transition-all group ${
              dropdownOpen
                ? 'bg-fill-2'
                : 'hover:bg-fill-2'
            }`}>
            <Plus
              theme='outline'
              size='13'
              fill='currentColor'
              className={`transition-transform duration-200 ${dropdownOpen ? 'rotate-45' : ''}`}
            />
          </button>
        </div>

        {expanded && tree.map((node) => (
          <WorkpathDrawer
            key={node.key}
            node={node}
            ui={ui}
            activeConversationId={activeConversationId}
            onCreateInteractive={handleCreateInteractive}
            onRemoveProjectWorkpath={handleRemoveProjectWorkpath}
            isProjectWorkpath={projectWorkpathKeys.has(node.key)}
            batchMode={batchMode}
            batchSelectionState={batchSelectionState}
            onToggleBatchSelectionScope={handleToggleBatchSelectionScope}
            renderEntry={renderEntry}
            displayPreferences={displayPreferences}
            gitBranch={workpathBranches.get(node.key)}
          />
        ))}

        {/* 空态提示已移除（导航精简） */}

        {/* Batch action bar — spans both session kinds */}
        {batchMode && (
          <BatchActionBar
            selectAllLabel={allSelected ? t('common.cancel') : t('conversation.history.selectAll')}
            onSelectAll={handleToggleSelectAll}
            actions={[
              {
                key: 'export',
                label: t('conversation.history.batchExport', { count: selectedConversationIds.size }),
                onClick: handleBatchExport,
                disabled: selectedConversationIds.size === 0,
              },
              {
                key: 'delete',
                label: t('conversation.history.batchDelete', { count: totalSelected }),
                onClick: handleBatchDeleteAll,
                danger: true,
                disabled: totalSelected === 0,
              },
            ]}
          />
        )}
      </div>
    </>
  );
};

export default WorkpathSessionList;
export { WorkpathSessionList };
