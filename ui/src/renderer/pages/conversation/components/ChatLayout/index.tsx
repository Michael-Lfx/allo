import type { ConversationId } from '@/common/types/ids';
import { conversationTarget } from '@/common/types/ids';
import { browserStorageKey } from '@/common/utils/browserStorageKey';
import type { PresetInfo } from '@/renderer/hooks/agent/usePresetInfo';
import appLogo from '@/renderer/assets/logo.svg';
import FlexFullContainer from '@/renderer/components/layout/FlexFullContainer';
import PathText from '@/renderer/components/base/PathText';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useResizableSplit } from '@/renderer/hooks/ui/useResizableSplit';
import ChatTitleEditor from '@/renderer/pages/conversation/components/ChatTitleEditor';
import {
  AgentTraceTrigger,
  SessionLogWorkspace,
  SessionLogsRoot,
  type ConversationColumnView,
} from '@/renderer/pages/conversation/components/AgentTraceInspector';
import KnowledgeControl from '@/renderer/pages/conversation/components/KnowledgeControl';
import { SummonHeaderBadge } from '@/renderer/pages/conversation/components/SummonPanel';
import WorkspacePanelHeader from './WorkspacePanelHeader';
import WorkspaceToolRail, {
  WORKSPACE_PANEL_META_EVENT,
  WORKSPACE_OPEN_PREVIEW_TOOL_EVENT,
  dispatchWorkspacePanelTabEvent,
  isWorkspacePanelEventForTarget,
  type WorkspacePanelMetaDetail,
  type WorkspacePanelTabDetail,
  type WorkspaceToolRailCollaboration,
} from './WorkspaceToolRail';
import { useContainerWidth } from '@/renderer/hooks/ui/useContainerWidth';
import { useTitleRename } from '@/renderer/pages/conversation/hooks/useTitleRename';
import { useWorkspacePanelTabs } from '@/renderer/pages/conversation/hooks/useWorkspacePanelTabs';
import {
  PreviewPanel,
  PreviewProvider,
  usePreviewContext,
  type WorkspacePreviewTabDefinition,
} from '@/renderer/pages/conversation/Preview';
import { WORKSPACE_TOGGLE_EVENT, type WorkspaceToggleDetail } from '@/renderer/utils/workspace/workspaceEvents';
import { ipcBridge } from '@/common';
import type { ITerminalSession } from '@/common/adapter/ipcBridge';
import { SHELL_SENTINEL } from '@/renderer/pages/terminal/launchPresets';
import { inferPreviewTabKind } from '@/renderer/pages/conversation/Preview/previewTabKind';
import classNames from 'classnames';
import {
  DEFAULT_WORKSPACE_PANEL_PX,
  MAX_WORKSPACE_PANEL_PX,
  MIN_WORKSPACE_PANEL_PX,
  WORKSPACE_HEADER_HEIGHT,
  calcLayoutMetrics,
} from '@/renderer/pages/conversation/utils/layoutCalc';
import {
  CHAT_HEADER_CLASSES,
  CHAT_HEADER_WITH_SUBTITLE_CLASSES,
} from '@/renderer/pages/conversation/components/conversationLayoutClasses';
import { Layout as ArcoLayout, Message } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { uuid } from '@/renderer/utils/common';
import type { WorkspaceExtraTab, WorkspaceTab } from '@/renderer/pages/conversation/Workspace/types';
import './chat-layout.css';

// headerExtra allows injecting custom actions (e.g., model picker) into the header's right area
export interface ChatLayoutProps {
  children?: React.ReactNode;
  title?: React.ReactNode;
  sider: React.ReactNode;
  siderTitle?: React.ReactNode;
  backend?: string;
  /** Preset info — when provided, the badge shows the preset identity instead of the backend. */
  preset?: PresetInfo;
  /** Fallback agent name (used when no preset, e.g. from conversation.extra.agent_name) */
  agent_name?: string;
  headerExtra?: React.ReactNode;
  /**
   * Hide the session-capability controls baked into the header
   * (Knowledge).
   * Used by surfaces that deliberately offer a reduced feature set — e.g. the
   * desktop companion chat tab. Defaults to false (full conversation page).
   */
  hideAdvancedControls?: boolean;
  /**
   * Make the header title read-only (no click-to-rename). Used by single-session
   * surfaces like the companion chat, where the title tracks an external source
   * (the companion name) and a per-conversation rename would desync it.
   */
  disableRename?: boolean;
  /**
   * 嵌套面板（如伙伴聊天 Tab）自带工作区开关：不依赖按路由门控的 app 标题栏。
   * 为 true 时，面板内折叠键与折叠态悬浮展开键无视桌面运行时一律渲染。默认 false，
   * 既有会话/终端表面行为不变（仍由标题栏驱动）。
   */
  selfContainedWorkspaceToggle?: boolean;
  workspaceEnabled?: boolean;
  /** Conversation ID for mode switching */
  conversation_id?: ConversationId;
  /** Custom tabs slot; when provided, replaces the default ConversationTabs */
  tabsSlot?: React.ReactNode;
  /** Workspace path for opening in external tools */
  workspacePath?: string;
  /** Real workspace path shown as a read-only subtitle below the title. */
  workspaceTitleSubtitle?: string;
  /** Authoritative temp-workspace flag from `conversation.extra.is_temporary_workspace`. */
  isTemporaryWorkspace?: boolean;
  /** Custom rename handler; when provided, replaces the default conversation.update rename flow */
  onRenameTitle?: (new_name: string) => Promise<boolean>;
  /** Optional override for the leading icon shown before the title. */
  headerLeading?: React.ReactNode;
  /** Extra panels exposed by the persistent vertical tool rail. */
  workspaceExtraTabs?: WorkspaceExtraTab[];
  /** Optional collaboration progress entry merged into the same tool rail. */
  workspaceCollaboration?: WorkspaceToolRailCollaboration;
}

/**
 * ChatLayoutInner — the actual chat surface layout. Lives strictly INSIDE the
 * per-surface {@link PreviewProvider} mounted by the {@link ChatLayout} wrapper,
 * so every `usePreviewContext()` consumer in this subtree (including this
 * component's own `isPreviewOpen` read, the `PreviewPanel`, the SendBoxes, the
 * workspace rail, MermaidBlock, …) resolves against THIS surface's provider.
 */
const ChatLayoutInner: React.FC<ChatLayoutProps> = (props) => {
  const { t } = useTranslation();
  const { conversation_id, workspacePath, isTemporaryWorkspace } = props;
  const workspaceTitleSubtitle = props.workspaceTitleSubtitle?.trim() || undefined;
  const hasWorkspaceTitleSubtitle = Boolean(workspaceTitleSubtitle);
  const titleSubtitle = workspaceTitleSubtitle ? (
    <span
      data-testid='conversation-workspace-subtitle'
      title={workspaceTitleSubtitle}
      className='block min-w-0 overflow-hidden text-11px leading-14px text-t-secondary'
    >
      <PathText path={workspaceTitleSubtitle} className='min-w-0' marqueeOnHover />
    </span>
  ) : undefined;
  const { active: developerMode } = useDeveloperModeGate();
  const [columnView, setColumnView] = useState<ConversationColumnView>('dialogue');
  const conversationViewKey = conversation_id ?? '';
  const conversationViewRef = useRef(conversationViewKey);
  if (conversationViewRef.current !== conversationViewKey) {
    conversationViewRef.current = conversationViewKey;
    if (columnView !== 'dialogue') {
      setColumnView('dialogue');
    }
  }
  const logsEnabled =
    developerMode === true && !props.hideAdvancedControls && conversation_id != null;
  const workspaceTarget = conversation_id != null ? conversationTarget(conversation_id) : undefined;
  const { workspaceEnabled = true } = props;
  const layout = useLayoutContext();
  const isDesktop = !layout?.isMobile;
  const isMobile = Boolean(layout?.isMobile);

  // Preview panel state
  const { isOpen: isPreviewOpen, activeTab, openWorkspaceTab, openTerminalTab, tabs } = usePreviewContext();
  const { activeWorkspaceTab, setActiveWorkspaceTab } = useWorkspacePanelTabs(workspaceTarget);
  const [creatingShell, setCreatingShell] = useState(false);

  const activeWorkspaceTitle =
    activeWorkspaceTab === 'files'
      ? props.siderTitle
      : activeWorkspaceTab === 'changes'
        ? t('conversation.workspace.changes.tab')
        : props.workspaceExtraTabs?.find((tab) => tab.key === activeWorkspaceTab)?.title ?? props.siderTitle;

  const workspaceTabs = useMemo<WorkspacePreviewTabDefinition[]>(() => {
    if (!workspaceEnabled) return [];
    return [
      { key: 'files', title: t('conversation.workspace.changes.filesTab') },
      { key: 'changes', title: t('conversation.workspace.changes.tab') },
      ...(props.workspaceExtraTabs
        ?.filter((tab) => tab.key !== 'conversation-terminals')
        .map((tab) => ({ key: tab.key, title: String(tab.title) })) ?? []),
    ];
  }, [props.workspaceExtraTabs, t, workspaceEnabled]);

  const openShellPreview = useCallback(async () => {
    const existing = tabs.find((tab) => inferPreviewTabKind(tab) === 'terminal' && tab.terminal_id);
    if (existing?.terminal_id) {
      openTerminalTab(
        { terminal_id: existing.terminal_id, name: existing.title } as ITerminalSession,
        { killOnClose: existing.killOnClose }
      );
      return;
    }
    if (!workspacePath) {
      Message.warning(t('preview.terminalNeedsWorkspace'));
      return;
    }
    if (creatingShell) return;
    setCreatingShell(true);
    try {
      const session = await ipcBridge.terminal.create.invoke({
        cwd: workspacePath,
        command: SHELL_SENTINEL,
        defer_spawn: true,
      });
      openTerminalTab(session, { killOnClose: true });
    } catch (error) {
      Message.error(error instanceof Error ? error.message : t('common.unknownError'));
    } finally {
      setCreatingShell(false);
    }
  }, [creatingShell, openTerminalTab, t, tabs, workspacePath]);

  const activateWorkspaceTool = useCallback(
    (tab: string) => {
      const nextTab = tab as WorkspaceTab;
      setActiveWorkspaceTab(nextTab);
      if (nextTab === 'conversation-terminals') {
        void openShellPreview();
        return;
      }
      const definition = workspaceTabs.find((candidate) => candidate.key === nextTab);
      if (definition) openWorkspaceTab(definition);
    },
    [openShellPreview, openWorkspaceTab, setActiveWorkspaceTab, workspaceTabs]
  );

  const selectWorkspaceTool = useCallback(
    (tab: string) => {
      if (workspaceTarget) dispatchWorkspacePanelTabEvent(tab as WorkspaceTab, workspaceTarget);
      activateWorkspaceTool(tab);
    },
    [activateWorkspaceTool, workspaceTarget]
  );

  // Header / auto-open use a dedicated event so rail clicks (which already call
  // selectWorkspaceTool) are not activated twice.
  useEffect(() => {
    if (typeof window === 'undefined' || !workspaceTarget || !workspaceEnabled) return undefined;
    const handleOpenPreviewTool = (event: Event) => {
      const detail = (event as CustomEvent<WorkspacePanelTabDetail>).detail;
      if (!isWorkspacePanelEventForTarget(detail?.target, workspaceTarget)) return;
      selectWorkspaceTool(detail.tab);
    };
    window.addEventListener(WORKSPACE_OPEN_PREVIEW_TOOL_EVENT, handleOpenPreviewTool);
    return () => window.removeEventListener(WORKSPACE_OPEN_PREVIEW_TOOL_EVENT, handleOpenPreviewTool);
  }, [selectWorkspaceTool, workspaceEnabled, workspaceTarget]);

  useEffect(() => {
    if (typeof window === 'undefined' || !workspaceTarget || !workspaceEnabled) return undefined;
    const handleWorkspaceToggle = (event: Event) => {
      const target = (event as CustomEvent<WorkspaceToggleDetail>).detail?.target;
      if (!target || target.kind !== workspaceTarget.kind || target.id !== workspaceTarget.id) return;
      selectWorkspaceTool('files');
    };
    window.addEventListener(WORKSPACE_TOGGLE_EVENT, handleWorkspaceToggle);
    return () => window.removeEventListener(WORKSPACE_TOGGLE_EVENT, handleWorkspaceToggle);
  }, [selectWorkspaceTool, workspaceEnabled, workspaceTarget]);

  const workspaceCollaboration = props.workspaceCollaboration
    ? {
        ...props.workspaceCollaboration,
        active:
          activeTab?.kind === 'workspace' && activeTab.workspaceTabKey === 'agent-execution',
        onClick: () => selectWorkspaceTool('agent-execution'),
      }
    : undefined;

  // --- Hook B: container width ---
  const { ref: containerRef, width: containerWidth } = useContainerWidth<HTMLDivElement>({
    fallbackToWindowWidth: true,
  });

  // --- Hook C: title rename ---
  const { editingTitle, setEditingTitle, titleDraft, setTitleDraft, renameLoading, canRenameTitle, submitTitleRename } =
    useTitleRename({
      title: props.title,
      conversation_id,
      onRename: props.onRenameTitle,
    });

  // The workspace is now a preview-tab surface, so the chat/preview split owns
  // all horizontal layout. Keep the existing calculator for its safe bounds.
  const { dynamicChatMinRatio, dynamicChatMaxRatio } = calcLayoutMetrics({
    containerWidth,
    workspaceWidthPx: 0,
    chatSplitRatio: 60, // placeholder; only dynamicChatMinRatio/dynamicChatMaxRatio are used here
    workspaceEnabled: false,
    isDesktop,
    isPreviewOpen,
    rightSiderCollapsed: true,
    isMobile,
  });

  const {
    splitRatio: chatSplitRatio,
    createDragHandle: createPreviewDragHandle,
  } = useResizableSplit({
    defaultWidth: 60,
    minWidth: dynamicChatMinRatio,
    maxWidth: dynamicChatMaxRatio,
    storageKey: 'chat-preview-split-ratio',
  });

  const { chatFlex, titleAreaMaxWidth } = calcLayoutMetrics({
    containerWidth,
    workspaceWidthPx: 0,
    chatSplitRatio,
    workspaceEnabled: false,
    isDesktop,
    isPreviewOpen,
    rightSiderCollapsed: true,
    isMobile,
  });

  const [mobileActionsSlot, setMobileActionsSlot] = useState<HTMLElement | null>(null);
  const [workspaceChangeCount, setWorkspaceChangeCount] = useState(0);
  useEffect(() => {
    if (!layout?.isMobile) {
      setMobileActionsSlot(null);
      return;
    }
    const findSlot = () => document.getElementById('app-titlebar-actions-slot');
    setMobileActionsSlot(findSlot());
    const observer = new MutationObserver(() => {
      const next = findSlot();
      setMobileActionsSlot((prev) => (prev === next ? prev : next));
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [layout?.isMobile]);

  useEffect(() => {
    if (typeof window === 'undefined') return undefined;
    const handleWorkspaceMeta = (event: Event) => {
      const detail = (event as CustomEvent<WorkspacePanelMetaDetail>).detail;
      if (
        !detail ||
        !workspaceTarget ||
        detail.target.kind !== workspaceTarget.kind ||
        detail.target.id !== workspaceTarget.id
      ) {
        return;
      }
      setWorkspaceChangeCount(detail.changeCount);
    };
    window.addEventListener(WORKSPACE_PANEL_META_EVENT, handleWorkspaceMeta);
    return () => window.removeEventListener(WORKSPACE_PANEL_META_EVENT, handleWorkspaceMeta);
  }, [workspaceTarget?.id, workspaceTarget?.kind]);

  const workspaceSider = useMemo(() => {
    if (!workspaceEnabled) return undefined;
    if (!React.isValidElement(props.sider)) return props.sider;
    return React.cloneElement(
      props.sider as React.ReactElement<{ extraTabs?: WorkspaceExtraTab[] }>,
      { extraTabs: props.workspaceExtraTabs }
    );
  }, [props.sider, props.workspaceExtraTabs, workspaceEnabled]);

  const isActiveWorkspaceTab =
    activeTab?.kind === 'workspace' && activeTab.workspaceTabKey === activeWorkspaceTab;
  const shellPreviewActive = activeTab?.kind === 'terminal';
  const showToolRail = workspaceEnabled && isDesktop;
  const workspaceHeader = activeWorkspaceTitle ? (
    <WorkspacePanelHeader
      showToggle={false}
      collapsed={false}
      onToggle={() => undefined}
      workspacePath={workspacePath}
      isTemporaryWorkspace={isTemporaryWorkspace}
      conversation_id={conversation_id}
      activeTab={activeWorkspaceTab}
    >
      {activeWorkspaceTitle}
    </WorkspacePanelHeader>
  ) : null;

  const desktopHeader = (
    <ArcoLayout.Header
      className={classNames(CHAT_HEADER_CLASSES, hasWorkspaceTitleSubtitle && CHAT_HEADER_WITH_SUBTITLE_CLASSES)}
    >
      <FlexFullContainer className='h-full min-w-0' containerClassName='flex items-center'>
        <ChatTitleEditor
          editingTitle={editingTitle}
          titleDraft={titleDraft}
          setTitleDraft={setTitleDraft}
          setEditingTitle={setEditingTitle}
          renameLoading={renameLoading}
          canRenameTitle={canRenameTitle && !props.disableRename}
          submitTitleRename={submitTitleRename}
          titleAreaMaxWidth={titleAreaMaxWidth}
          title={props.title}
          subtitle={titleSubtitle}
          conversation_id={conversation_id}
          leading={
            props.headerLeading ??
            (<img src={appLogo} alt='Flowy' className='block h-16px w-16px object-contain' />)
          }
        />
      </FlexFullContainer>
      <div className='flex items-center gap-12px shrink-0'>
        {!props.hideAdvancedControls && conversation_id != null && (
          <>
            {/* 召唤伙伴徽标（设计 B5）：仅已召唤会话渲染，被动展示伙伴名。 */}
            <SummonHeaderBadge conversationId={conversation_id} />
            {logsEnabled ? <AgentTraceTrigger /> : null}
            <KnowledgeControl target={{ kind: 'conversation', id: conversation_id }} />
          </>
        )}
        {props.headerExtra}
      </div>
    </ArcoLayout.Header>
  );

  const headerBlock = (
    <>
      {layout?.isMobile
        ? mobileActionsSlot && props.headerExtra && createPortal(props.headerExtra, mobileActionsSlot)
        : desktopHeader}
      {props.tabsSlot}
    </>
  );

  const chatColumnBody = logsEnabled ? (
    <div className='conversation-column-slide'>
      <div className='conversation-column-viewport'>
        <div
          className={classNames(
            'conversation-column-track',
            columnView === 'logs' && 'conversation-column-track--logs'
          )}
        >
          <div
            className={classNames(
              'conversation-column-pane',
              columnView === 'dialogue' && 'is-active'
            )}
            inert={columnView !== 'dialogue' ? true : undefined}
            aria-hidden={columnView !== 'dialogue'}
          >
            <ArcoLayout.Content className='flex flex-col flex-1 bg-1 overflow-hidden h-full'>
              {props.children}
            </ArcoLayout.Content>
          </div>
          <div
            className={classNames('conversation-column-pane', columnView === 'logs' && 'is-active')}
            inert={columnView !== 'logs' ? true : undefined}
            aria-hidden={columnView !== 'logs'}
          >
            <SessionLogWorkspace />
          </div>
        </div>
      </div>
    </div>
  ) : (
    <ArcoLayout.Content className='flex flex-col flex-1 bg-1 overflow-hidden'>
      {props.children}
    </ArcoLayout.Content>
  );

  const layoutTree = (
    <ArcoLayout
      className='size-full color-black '
      style={{
        // fontFamily: `cursive,"anthropicSans","anthropicSans Fallback",system-ui,Segoe UI,Roboto,Helvetica,Arial,sans-serif`,
      }}
    >
      <div ref={containerRef} className='flex flex-1 relative w-full overflow-hidden'>
        {/* Unified layout: single DOM structure prevents children unmount/remount on preview toggle.
            Desktop: header lives in the chat column so the preview strip is flush with the top.
            Mobile: column stack keeps the header visible when preview takes the remaining height. */}
        <div
          className={classNames('flex min-w-0 min-h-0', isMobile ? 'flex-col' : 'flex-row')}
          style={{
            flexGrow: 1,
            flexShrink: 1,
            flexBasis: 0,
          }}
        >
          <div
            className='flex flex-col min-w-0 min-h-0'
            style={{
              flexGrow: isPreviewOpen ? 0 : 1,
              flexShrink: isDesktop ? 0 : isPreviewOpen ? 0 : 1,
              flexBasis: isPreviewOpen && isDesktop ? `${chatFlex}%` : isDesktop ? 0 : 'auto',
              minWidth: isDesktop ? '240px' : undefined,
            }}
          >
            <div className='shrink-0 !bg-1'>{headerBlock}</div>
            {/* Chat area - always mounted, never unmounted on preview toggle */}
            <div
              className='flex flex-col relative min-h-0 flex-1'
              style={{
                display: isPreviewOpen && isMobile ? 'none' : 'flex',
              }}
            >
              {chatColumnBody}
            </div>
          </div>
          <div
              className={classNames(
                'preview-panel flex flex-col relative min-w-0 overflow-visible',
                isDesktop
                  ? showToolRail
                    ? 'mb-[12px] ml-[8px] rounded-bl-[15px]'
                    : 'mb-[12px] mr-[12px] ml-[8px] rounded-b-[15px]'
                  : 'm-[8px] rounded-[15px]'
              )}
              style={{
                flexGrow: 1,
                flexShrink: 1,
                flexBasis: 0,
                border: '1px solid var(--bg-3)',
                borderRight: showToolRail ? 'none' : undefined,
                minWidth: isDesktop ? '260px' : 0,
                minHeight: 0,
                maxWidth: isMobile ? 'calc(100% - 16px)' : undefined,
                width: isMobile ? 'calc(100% - 16px)' : undefined,
                boxSizing: 'border-box',
                display: isPreviewOpen ? undefined : 'none',
              }}
            >
              {isDesktop &&
                createPreviewDragHandle({
                  className: 'absolute top-0 bottom-0 z-30',
                  style: { width: '20px', left: '-20px' },
                  linePlacement: 'end',
                  lineClassName: 'opacity-30 group-hover:opacity-100 group-active:opacity-100',
                  lineStyle: { width: '2px' },
                })}
              <div
                className={classNames(
                  'h-full w-full overflow-hidden',
                  isDesktop
                    ? showToolRail
                      ? 'rounded-bl-[15px]'
                      : 'rounded-b-[15px]'
                    : 'rounded-[15px]'
                )}
              >
                <PreviewPanel
                  workspaceContent={workspaceSider}
                  workspaceTabs={workspaceTabs}
                  renderWorkspaceHeader={() => workspaceHeader}
                  onWorkspaceTabActivate={selectWorkspaceTool}
                />
              </div>
          </div>
        </div>
        {workspaceEnabled && !layout?.isMobile && (
          <WorkspaceToolRail
            t={t}
            activeTab={activeWorkspaceTab}
            expanded={isActiveWorkspaceTab}
            shellPreviewActive={shellPreviewActive}
            onSelect={selectWorkspaceTool}
            changeCount={workspaceChangeCount}
            extraTabs={props.workspaceExtraTabs}
            collaboration={workspaceCollaboration}
          />
        )}
      </div>
    </ArcoLayout>
  );

  if (logsEnabled && conversation_id) {
    return (
      <SessionLogsRoot
        conversationId={conversation_id}
        view={columnView}
        onViewChange={setColumnView}
      >
        {layoutTree}
      </SessionLogsRoot>
    );
  }
  return layoutTree;
};

/**
 * ChatLayout — per-surface chat layout. Mounts its OWN {@link PreviewProvider}
 * so the file/diff preview state is scoped to this surface instead of a global
 * singleton (which used to leak preview tabs across views and required the
 * three `closePreview()` cleanup calls in Sider / ConversationShell /
 * conversation index that have since been removed). The provider unmounts with
 * the surface, so cross-view leak can no longer happen.
 *
 * The persistence namespace includes the conversation id, so switching
 * conversations restores only that conversation's tabs. `subscribeGlobalOpen`
 * lets agent/MCP `preview.open` events open a preview on the conversation
 * surface (the primary surface).
 */
const ChatLayout: React.FC<ChatLayoutProps> = (props) => {
  const pendingPreviewScope = useRef(`conversation-pending:${uuid()}`);
  const previewScope = props.conversation_id
    ? browserStorageKey('workspace-preview', 'conversation', props.conversation_id)
    : pendingPreviewScope.current;

  return (
    <PreviewProvider
      key={previewScope}
      persistNamespace={previewScope}
      subscribeGlobalOpen={true}
      workspacePath={props.workspacePath}
    >
      <ChatLayoutInner {...props} />
    </PreviewProvider>
  );
};

export default ChatLayout;
