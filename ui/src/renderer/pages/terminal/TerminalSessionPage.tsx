

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Navigate, useParams } from 'react-router-dom';
import { Button, Input, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Refresh, EditOne, Terminal } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import type { ITerminalSession } from '@/common/adapter/ipcBridge';
import { terminalTarget, tryParseEntityId, type TerminalId } from '@/common/types/ids';
import { browserStorageKey } from '@/common/utils/browserStorageKey';
import AutoWorkControl from '@/renderer/pages/conversation/components/AutoWorkControl';
import IdmmControl from '@/renderer/pages/conversation/components/IdmmControl';
import KnowledgeControl from '@/renderer/pages/conversation/components/KnowledgeControl';
import { useResizableSplit } from '@/renderer/hooks/ui/useResizableSplit';
import {
  PreviewPanel,
  PreviewProvider,
  usePreviewContext,
  type WorkspacePreviewTabDefinition,
} from '@/renderer/pages/conversation/Preview';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { isDesktopShell } from '@/renderer/utils/platform';
import WorkspacePanelHeader from '@/renderer/pages/conversation/components/ChatLayout/WorkspacePanelHeader';
import WorkspaceToolRail, {
  WORKSPACE_PANEL_META_EVENT,
  dispatchWorkspacePanelTabEvent,
  type WorkspacePanelMetaDetail,
} from '@/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail';
import { useWorkspacePanelTabs } from '@/renderer/pages/conversation/hooks/useWorkspacePanelTabs';
import { WORKSPACE_TOGGLE_EVENT, type WorkspaceToggleDetail } from '@/renderer/utils/workspace/workspaceEvents';
import RegisterKnowledgeButton from './RegisterKnowledgeButton';
import TerminalWorkspaceRail from './TerminalWorkspaceRail';
import XtermView, { type XtermViewHandle } from './XtermView';
import TerminalSendBox from './TerminalSendBox';
import { isTerminalAutoworkCapable } from './detectFamily';
import styles from './XtermView.module.css';

/** Preview column minimum width (px) so it never collapses to nothing. */
const TERMINAL_PREVIEW_MIN_PX = 260;

type TerminalLoadError = 'not-found' | 'request-failed';

/** Terminal preview and workspace-tab navigation, scoped to one terminal session. */
const TerminalRightRegion: React.FC<{ session: ITerminalSession }> = ({ session }) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = Boolean(layout?.isMobile);
  const { isOpen: isPreviewOpen, activeTab, openWorkspaceTab } = usePreviewContext();
  const workspaceTarget = terminalTarget(session.terminal_id);
  const { activeWorkspaceTab, setActiveWorkspaceTab } = useWorkspacePanelTabs(workspaceTarget);
  const [workspaceChangeCount, setWorkspaceChangeCount] = useState(0);
  const workspaceTabs = useMemo<WorkspacePreviewTabDefinition[]>(
    () => [
      { key: 'files', title: t('conversation.workspace.changes.filesTab') },
      { key: 'changes', title: t('conversation.workspace.changes.tab') },
    ],
    [t]
  );

  useEffect(() => {
    const handleMeta = (event: Event) => {
      const detail = (event as CustomEvent<WorkspacePanelMetaDetail>).detail;
      if (
        detail?.target?.kind === workspaceTarget.kind &&
        detail.target.id === workspaceTarget.id
      ) {
        setWorkspaceChangeCount(detail.changeCount);
      }
    };
    window.addEventListener(WORKSPACE_PANEL_META_EVENT, handleMeta);
    return () => window.removeEventListener(WORKSPACE_PANEL_META_EVENT, handleMeta);
  }, [workspaceTarget.id, workspaceTarget.kind]);

  const selectWorkspaceTool = useCallback((tab: string) => {
    setActiveWorkspaceTab(tab);
    dispatchWorkspacePanelTabEvent(tab, workspaceTarget);
    const definition = workspaceTabs.find((candidate) => candidate.key === tab);
    if (definition) openWorkspaceTab(definition);
  }, [openWorkspaceTab, setActiveWorkspaceTab, workspaceTabs, workspaceTarget]);

  useEffect(() => {
    if (typeof window === 'undefined') return undefined;
    const handleWorkspaceToggle = (event: Event) => {
      const target = (event as CustomEvent<WorkspaceToggleDetail>).detail?.target;
      if (!target || target.kind !== workspaceTarget.kind || target.id !== workspaceTarget.id) return;
      selectWorkspaceTool('files');
    };
    window.addEventListener(WORKSPACE_TOGGLE_EVENT, handleWorkspaceToggle);
    return () => window.removeEventListener(WORKSPACE_TOGGLE_EVENT, handleWorkspaceToggle);
  }, [selectWorkspaceTool, workspaceTarget]);

  // Preview column width (px), persisted independently of the rail.
  const { splitRatio: previewWidthPx, createDragHandle: createPreviewDragHandle } = useResizableSplit({
    unit: 'px',
    defaultWidth: 420,
    minWidth: TERMINAL_PREVIEW_MIN_PX,
    maxWidth: 960,
    storageKey: 'terminal-preview-width-px',
  });

  return (
    <>
      {/* Keep the workspace host mounted while its preview column is hidden so
          file snapshots and change badges continue updating before first open. */}
      <div
        className='relative flex flex-col min-h-0 bg-1'
        style={{
          flex: `0 0 ${Math.round(previewWidthPx)}px`,
          width: `${Math.round(previewWidthPx)}px`,
          minWidth: `${TERMINAL_PREVIEW_MIN_PX}px`,
          display: isPreviewOpen ? undefined : 'none',
        }}
      >
        {isPreviewOpen && createPreviewDragHandle({
            className: 'absolute top-0 bottom-0 left-0 z-30',
            style: { width: '12px', left: '-6px' },
            reverse: true,
            linePlacement: 'start',
          })}
        <div className='h-full w-full overflow-hidden'>
          <PreviewPanel
            workspaceContent={<TerminalWorkspaceRail session={session} />}
            workspaceTabs={workspaceTabs}
            onWorkspaceTabActivate={selectWorkspaceTool}
            renderWorkspaceHeader={() => (
              <WorkspacePanelHeader
                showToggle={false}
                collapsed={false}
                onToggle={() => undefined}
                workspacePath={session.cwd}
                activeTab={activeWorkspaceTab}
              >
                <span className='text-14px font-medium text-t-primary truncate'>
                  {activeWorkspaceTab === 'changes'
                    ? t('conversation.workspace.changes.tab')
                    : t('terminal.workspace.title', { defaultValue: '项目' })}
                </span>
              </WorkspacePanelHeader>
            )}
          />
        </div>
      </div>

      {!isMobile && (
        <WorkspaceToolRail
          t={t}
          activeTab={activeWorkspaceTab}
          expanded={activeTab?.kind === 'workspace' && activeTab.workspaceTabKey === activeWorkspaceTab}
          onSelect={selectWorkspaceTool}
          changeCount={workspaceChangeCount}
        />
      )}
    </>
  );
};

const TerminalSessionContent: React.FC<{ sessionId: TerminalId }> = ({ sessionId }) => {
  const { t } = useTranslation();
  const [session, setSession] = useState<ITerminalSession | null>(null);
  const [loadError, setLoadError] = useState<TerminalLoadError | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [terminalError, setTerminalError] = useState<Error | null>(null);
  const [xtermAttempt, setXtermAttempt] = useState(0);
  const [relaunching, setRelaunching] = useState(false);
  const [fallingBack, setFallingBack] = useState(false);
  const fallingBackRef = useRef(false);
  const xtermApi = useRef<XtermViewHandle | null>(null);
  // Inline title editing in the header.
  const [editingName, setEditingName] = useState(false);
  const [draftName, setDraftName] = useState('');
  const [savingName, setSavingName] = useState(false);
  const savingNameRef = useRef(false);
  const skipBlurSaveRef = useRef(false);

  useEffect(() => {
    let active = true;
    setSession(null);
    setLoadError(null);
    void ipcBridge.terminal.get
      .invoke({ terminal_id: sessionId })
      .then((s) => {
        if (!active) return;
        if (s) {
          setSession(s);
          return;
        }
        setLoadError('not-found');
      })
      .catch((error: unknown) => {
        if (!active) return;
        console.error('[TerminalSessionPage] Failed to load terminal session:', error);
        setLoadError('request-failed');
      });

    const offExit = ipcBridge.terminal.onExit.on((evt) => {
      if (evt.terminal_id === sessionId)
        setSession((prev) => (prev ? { ...prev, last_status: 'exited', exit_code: evt.exit_code } : prev));
    });
    const offUpdated = ipcBridge.terminal.onUpdated.on((s) => {
      if (s.terminal_id === sessionId) {
        setLoadError(null);
        if (s.last_status === 'running') setTerminalError(null);
        setSession(s);
      }
    });
    return () => {
      active = false;
      offExit();
      offUpdated();
    };
  }, [loadAttempt, sessionId]);

  const handleRelaunch = useCallback(async () => {
    if (!session) return;
    setRelaunching(true);
    try {
      // Relaunch in place: the backend respawns the PTY for the SAME session id
      // (a PTY child cannot be resumed once it exits, so a fresh process is
      // unavoidable, but reusing the id keeps this tab/session continuous —
      // no new sidebar entry, no session sprawl). Clear the stale output first;
      // the new process's output streams over the same WS subscription.
      const updated = await ipcBridge.terminal.relaunch.invoke({
        terminal_id: session.terminal_id,
      });
      xtermApi.current?.clear();
      xtermApi.current?.focus();
      setSession(updated);
      setTerminalError(null);
      setXtermAttempt((attempt) => attempt + 1);
    } catch (err) {
      Message.error(err instanceof Error ? err.message : String(err));
    } finally {
      setRelaunching(false);
    }
  }, [session]);

  // Fall back to a clean login shell in place: the escape hatch for a wedged,
  // garbled claude/codex TUI. Kills the agent and respawns the platform shell
  // under the SAME session id, then full-resets the xterm grid (exits alt-screen
  // / clears the garble). Wired to both the header button and the rapid-Ctrl+C
  // escalation. Guarded so a Ctrl+C burst + a button click cannot double-fire.
  const handleFallbackShell = useCallback(async () => {
    if (!session || fallingBackRef.current) return;
    fallingBackRef.current = true;
    setFallingBack(true);
    try {
      const updated = await ipcBridge.terminal.relaunchShell.invoke({ terminal_id: session.terminal_id });
      xtermApi.current?.reset();
      xtermApi.current?.focus();
      setSession(updated);
      setTerminalError(null);
      setXtermAttempt((attempt) => attempt + 1);
      Message.success(t('terminal.fallbackShellDone'));
    } catch (err) {
      Message.error(err instanceof Error ? err.message : String(err));
    } finally {
      fallingBackRef.current = false;
      setFallingBack(false);
    }
  }, [session, t]);

  const startEditName = useCallback(() => {
    if (!session) return;
    setDraftName(session.name ?? '');
    setEditingName(true);
  }, [session]);

  // Save the edited title via the same update API the sidebar rename uses; the
  // sidebar stays in sync through its own `terminal.updated` subscription.
  const saveName = useCallback(async () => {
    if (savingNameRef.current || !session) return;
    const trimmed = draftName.trim();
    // Empty or unchanged → treat as cancel; no request.
    if (!trimmed || trimmed === session.name) {
      setEditingName(false);
      return;
    }
    savingNameRef.current = true;
    setSavingName(true);
    try {
      const updated = await ipcBridge.terminal.update.invoke({
        terminal_id: session.terminal_id,
        name: trimmed,
      });
      setSession(updated);
      // Mirror cancelEditName: the unmount-triggered blur must not re-save.
      skipBlurSaveRef.current = true;
      setEditingName(false);
    } catch (err) {
      Message.error(err instanceof Error ? err.message : String(err));
    } finally {
      savingNameRef.current = false;
      setSavingName(false);
    }
  }, [session, draftName]);

  // Blur commits the edit — except right after Esc, which cancels.
  const handleNameBlur = useCallback(() => {
    if (skipBlurSaveRef.current) {
      skipBlurSaveRef.current = false;
      return;
    }
    void saveName();
  }, [saveName]);

  const cancelEditName = useCallback(() => {
    skipBlurSaveRef.current = true;
    setEditingName(false);
  }, []);

  if (!session) {
    return (
      <div className='flex h-full w-full items-center justify-center bg-fill-1 px-24px'>
        {loadError ? (
          <div role='alert' className='flex max-w-560px flex-col items-center gap-12px text-center'>
            <div className='text-14px text-t-primary'>
              {loadError === 'not-found'
                ? t('terminal.loadNotFound', { defaultValue: 'Terminal session not found.' })
                : t('terminal.loadFailed', { defaultValue: 'Failed to load terminal session.' })}
            </div>
            <Button type='primary' onClick={() => setLoadAttempt((attempt) => attempt + 1)}>
              {t('common.retry', { defaultValue: 'Retry' })}
            </Button>
          </div>
        ) : (
          <Spin />
        )}
      </div>
    );
  }

  const isExited = session.last_status !== 'running';

  // AutoWork is only meaningful for agent-CLI terminals running in the foreground.
  // Capability is resolved from the launch command/args/backend the SAME way the
  // backend gate does (wrappers like `stepcode claude` count; a plain shell or
  // gemini does not).
  const isAgentCli = isTerminalAutoworkCapable(session?.command ?? '', session?.args ?? [], session?.backend);
  const autoWorkDisabledReason = !isAgentCli
    ? t('terminal.autowork.requiresAgentCli')
    : isExited
      ? t('terminal.autowork.terminalExited')
      : undefined;
  const needsFullAuto = isAgentCli && session?.mode !== 'full-auto';
  const autoWorkSafetyHint = needsFullAuto ? t('terminal.autowork.fullAutoHint') : undefined;
  const autoWorkBlockEnable = needsFullAuto ? t('terminal.autowork.fullAutoRequired') : undefined;

  return (
    // The WHOLE page (both columns) is wrapped in the terminal-scoped
    // PreviewProvider — not just the right region. TerminalSendBox (left column)
    // reuses the shared chat SendBox, which calls usePreviewContext()
    // (setSendBoxHandler). The global app-level PreviewProvider was removed in
    // favor of per-surface providers, so wrapping only the right region left the
    // SendBox with no provider in scope → "usePreviewContext must be used within
    // PreviewProvider" → white screen on terminal mount. subscribeGlobalOpen=
    // false keeps agent-driven global preview.open out of the terminal; the
    // The terminal id is part of the namespace so separate terminal sessions
    // never restore or overwrite each other's preview tabs.
    <PreviewProvider
      persistNamespace={browserStorageKey('workspace-preview', 'terminal', sessionId)}
      subscribeGlobalOpen={false}
      workspacePath={session.cwd}
    >
    <div className='relative flex flex-row h-full min-h-0 bg-fill-1 overflow-hidden'>
      {/* Terminal column: header + xterm + composer. flex-1 with a floor so it
          never collapses when the preview / rail columns open. */}
      <div className='flex flex-col flex-1 min-w-0 h-full' style={{ minWidth: 360 }}>
        {/* Header */}
        <div className={`${styles.header} flex items-center justify-between px-16px py-10px`}>
          <div className='flex items-center gap-8px min-w-0'>
            {editingName ? (
              <Input
                size='small'
                autoFocus
                disabled={savingName}
                value={draftName}
                onChange={setDraftName}
                onPressEnter={() => void saveName()}
                onBlur={handleNameBlur}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') {
                    e.preventDefault();
                    cancelEditName();
                  }
                }}
                className='w-240px max-w-full'
              />
            ) : (
              <div
                className='group flex items-center gap-4px min-w-0 cursor-text'
                onClick={startEditName}
                title={t('terminal.action.rename')}
              >
                <span className='text-14px font-medium text-t-primary truncate'>
                  {session?.name || t('terminal.untitled')}
                </span>
                <EditOne className='shrink-0 opacity-0 group-hover:opacity-60 transition-opacity' size='14' />
              </div>
            )}
            {isExited && !editingName && (
              <span className='text-12px text-t-tertiary'>
                {t('terminal.statusExited', {
                  code: String(session?.exit_code ?? 0),
                })}
              </span>
            )}
          </div>
          <div className='flex items-center gap-8px shrink-0'>
            <KnowledgeControl
              target={{ kind: 'terminal', id: sessionId }}
              applyNote={t('terminal.knowledge.applyLiveNote')}
              footer={
                // External-CLI registration footer is desktop-host-only
                // (audit 2026-07-30, finding I).
                isDesktopShell() ? (
                  <div className='flex flex-col gap-6px'>
                    <span className='text-11px leading-15px text-t-tertiary'>
                      {t('terminal.extended.knowledgeConnectNote', {
                        defaultValue:
                          '外置 CLI 可注册无密钥命令；启动时由当前系统用户专属的本地安全通道授权。',
                      })}
                    </span>
                    <RegisterKnowledgeButton cwd={session?.cwd ?? ''} command={session?.command ?? ''} />
                  </div>
                ) : undefined
              }
            />
            <AutoWorkControl
              target={{ kind: 'terminal', id: sessionId }}
              disabledReason={autoWorkDisabledReason}
              safetyHint={autoWorkSafetyHint}
              blockEnableReason={autoWorkBlockEnable}
            />
            <IdmmControl target={{ kind: 'terminal', id: sessionId }} />
            {/* Escape hatch for a wedged/garbled claude/codex TUI: always
                available (NOT gated on isExited) for agent sessions. After a
                fallback the session is a plain shell, so isAgentCli flips false
                and this button disappears. */}
            {isAgentCli && (
              <Button
                size='small'
                loading={fallingBack}
                className='flowy-icon-text-btn'
                icon={<Terminal size='14' />}
                onClick={handleFallbackShell}
                title={t('terminal.fallbackShellTip')}
              >
                {t('terminal.fallbackShell')}
              </Button>
            )}
            {isExited && (
              <Button
                type='primary'
                size='small'
                loading={relaunching}
                className='flowy-icon-text-btn'
                icon={<Refresh size='14' />}
                onClick={handleRelaunch}
              >
                {t('terminal.relaunch')}
              </Button>
            )}
          </div>
        </div>

        {/* Terminal output */}
        <div className='flex-1 min-h-0 px-12px pt-12px'>
          {terminalError ? (
            <div role='alert' className={`${styles.card} flex h-full items-center justify-center px-24px`}>
              <div className='flex max-w-560px flex-col items-center gap-12px text-center'>
                <div className='text-14px text-t-primary'>
                  {t('terminal.activationFailed', { defaultValue: 'Failed to activate terminal.' })}
                </div>
                <div className='max-w-full select-text break-all text-12px text-t-tertiary'>
                  {terminalError.message}
                </div>
                <Button
                  type='primary'
                  onClick={() => {
                    setTerminalError(null);
                    setXtermAttempt((attempt) => attempt + 1);
                  }}
                >
                  {t('common.retry', { defaultValue: 'Retry' })}
                </Button>
              </div>
            </div>
          ) : (
            <XtermView
              key={`${sessionId}:${xtermAttempt}`}
              sessionId={sessionId}
              isRunning={!isExited}
              apiRef={xtermApi}
              className='h-full'
              onEscalateShell={isAgentCli ? handleFallbackShell : undefined}
              onResizeFailure={(error: unknown) =>
                setTerminalError(error instanceof Error ? error : new Error(String(error)))
              }
            />
          )}
        </div>

        {/* Enhanced composer */}
        <div className='px-12px pt-8px pb-12px'>
          <TerminalSendBox
            terminalApi={xtermApi}
            disabled={isExited || terminalError !== null}
            onClearView={() => xtermApi.current?.clear()}
          />
        </div>
      </div>

      {/* Right region: preview + workspace rail. Lives inside the page-level
          PreviewProvider above. Mounted only once the session is loaded (the
          rail needs session.terminal_id / session.cwd). */}
      {session && <TerminalRightRegion session={session} />}
    </div>
    </PreviewProvider>
  );
};

const TerminalSessionPage: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  if (!id) return <Navigate to='/guid' replace />;

  const sessionId = tryParseEntityId('terminal', id);
  if (!sessionId) return <Navigate to='/guid' replace />;
  // React Router reuses the same route element when only `:id` changes. Keying
  // the stateful page content prevents session A's loaded metadata, edit state,
  // or in-flight actions from being rendered against terminal B.
  return <TerminalSessionContent key={sessionId} sessionId={sessionId} />;
};

export default TerminalSessionPage;
