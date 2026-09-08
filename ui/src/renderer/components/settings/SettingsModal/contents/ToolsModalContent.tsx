
import type { IMcpServer } from '@/common/config/storage';
import { getAgents } from '@/renderer/hooks/agent/useAgents';
import { Button, Dropdown, Menu, Modal } from '@arco-design/web-react';
import type { AppMessageInstance } from '@/renderer/components/notifications';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { Check, CloseSmall, Down, Plus } from '@icon-park/react';
import React, { useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import SettingsContentLoading from '@/renderer/components/layout/SettingsContentLoading';
import AddMcpServerModal from '@/renderer/pages/settings/components/AddMcpServerModal';
import ExtensionMcpServerItem from '@/renderer/pages/settings/ToolsSettings/ExtensionMcpServerItem';
import McpServerItem from '@/renderer/pages/settings/ToolsSettings/McpServerItem';
import McpLoadingIndicator from '@/renderer/pages/settings/ToolsSettings/McpLoadingIndicator';
import { useMcpServers, useMcpConnection, useMcpModal, useMcpServerCRUD, useMcpOAuth } from '@/renderer/hooks/mcp';
import {
  extensionMcpUiKey,
  mcpServerUiKey,
  type ExtensionMcpServerContribution,
} from '@/renderer/hooks/mcp/extensionCatalog';
import {
  useMcpActivationFlow,
  type McpActivationItemState,
  type McpActivationNavigationState,
  type McpActivationProgress,
} from '@/renderer/hooks/mcp/useMcpActivationFlow';

type MessageInstance = AppMessageInstance;

export type McpInstalledPanelHandle = {
  openAdd: (mode: 'json' | 'oneclick') => void;
};

/** One-shot activation hand-off from the market "add and enable" flow. */
export type McpPendingActivation = McpActivationNavigationState;

type McpConnectionTesters = {
  handleTestMcpConnection: (server: IMcpServer, options?: { notify?: boolean }) => Promise<void>;
  handleTestMcpConnections: (
    servers: IMcpServer[],
    options?: { notify?: boolean; concurrency?: number }
  ) => Promise<void>;
};

function McpAddChrome({
  onOpenJson,
  onOpenOneClick,
}: {
  onOpenJson: () => void;
  onOpenOneClick: () => void;
}) {
  const { t } = useTranslation();
  const [detectedAgents, setDetectedAgents] = useState<Array<{ backend: string; name: string }>>([]);

  useEffect(() => {
    void getAgents()
      .then((agents) => {
        setDetectedAgents(agents.map((agent) => ({ backend: agent.backend ?? '', name: agent.name })));
      })
      .catch((error: unknown) => {
        console.error('Failed to load agents:', error);
      });
  }, []);

  if (detectedAgents.length > 0) {
    return (
      <Dropdown
        trigger='click'
        droplist={
          <Menu>
            <Menu.Item
              key='json'
              onClick={(e) => {
                e.stopPropagation();
                onOpenJson();
              }}
            >
              {t('settings.mcpImportFromJSON')}
            </Menu.Item>
            <Menu.Item
              key='oneclick'
              onClick={(e) => {
                e.stopPropagation();
                onOpenOneClick();
              }}
            >
              {t('settings.mcpOneKeyImport')}
            </Menu.Item>
          </Menu>
        }
      >
        <Button type='outline' className='flowy-icon-text-btn' icon={<Plus size={'16'} />} shape='round' onClick={(e) => e.stopPropagation()}>
          {t('settings.mcpAddServer')} <Down size='12' />
        </Button>
      </Dropdown>
    );
  }

  return (
    <Button
      type='outline'
      className='flowy-icon-text-btn'
      icon={<Plus size={'16'} />}
      shape='round'
      onClick={onOpenJson}
    >
      {t('settings.mcpAddServer')}
    </Button>
  );
}

function McpActivationNotice({
  progress,
  states,
  servers,
}: {
  progress?: McpActivationProgress;
  states: Record<string, McpActivationItemState>;
  servers: IMcpServer[];
}) {
  const { t } = useTranslation();
  if (!progress) return null;

  const currentServer = progress.currentServerId
    ? servers.find((server) => server.mcp_server_id === progress.currentServerId)
    : undefined;
  const isRunning = progress.completed < progress.total;
  const failedCount = Object.values(states).filter(
    (state) => state === 'failed' || state === 'needs-auth' || state === 'config-changed'
  ).length;

  return (
    <div
      className='mx-auto flex w-full max-w-1180px items-center justify-between gap-16px rd-12px border border-solid border-arco-4 bg-[rgba(var(--arcoblue-1),0.18)] px-16px py-12px'
      role='status'
      aria-live='polite'
      aria-busy={isRunning || undefined}
    >
      <div className='flex min-w-0 items-start gap-10px'>
        <span className='mt-2px inline-flex h-20px w-20px shrink-0 items-center justify-center' aria-hidden='true'>
          {isRunning ? <McpLoadingIndicator size='medium' /> : failedCount > 0 ? <CloseSmall size='16' /> : <Check size='16' />}
        </span>
        <div className='min-w-0'>
          <div className='text-13px font-medium text-t-primary'>
            {isRunning
              ? t('settings.mcpActivationProgress', {
                  completed: progress.completed,
                  total: progress.total,
                })
              : failedCount > 0
                ? t('settings.mcpActivationFinishedWithErrors', { count: failedCount })
                : t('settings.mcpActivationFinished')}
          </div>
          <div className='mt-2px truncate text-12px text-t-secondary'>
            {isRunning && currentServer
              ? t('settings.mcpActivationCurrent', { name: currentServer.name })
              : isRunning
                ? t('settings.mcpActivationWaiting')
                : t('settings.mcpActivationFinishedHint')}
          </div>
        </div>
      </div>
      {isRunning ? (
        <span className='shrink-0 text-12px tabular-nums text-t-secondary'>
          {progress.completed}/{progress.total}
        </span>
      ) : null}
    </div>
  );
}

function McpInstalledList({
  message,
  mcpServers,
  extensionMcpServers,
  setMcpServers,
  searchQuery,
  mcpCollapseKey,
  toggleServerCollapse,
  showEditMcpModal,
  showDeleteConfirm,
  onToggleEnabled,
  testersRef,
  isMcpServersLoading,
  mcpServersLoadFailed,
  reloadMcpServers,
  activationStates,
  activationErrors,
}: {
  message: MessageInstance;
  mcpServers: IMcpServer[];
  extensionMcpServers: ExtensionMcpServerContribution[];
  setMcpServers: React.Dispatch<React.SetStateAction<IMcpServer[]>>;
  searchQuery: string;
  mcpCollapseKey: Record<string, boolean>;
  toggleServerCollapse: (uiKey: string) => void;
  showEditMcpModal: (server: IMcpServer) => void;
  showDeleteConfirm: (serverId: IMcpServer['mcp_server_id']) => void;
  onToggleEnabled: (server: IMcpServer) => Promise<IMcpServer | undefined>;
  testersRef: React.RefObject<McpConnectionTesters | null>;
  isMcpServersLoading: boolean;
  mcpServersLoadFailed: boolean;
  reloadMcpServers?: () => void;
  activationStates: Record<string, McpActivationItemState>;
  activationErrors: Record<string, string>;
}) {
  const { t } = useTranslation();
  const { oauthStatus, loggingIn, checkOAuthStatus, markLoginRequired, clearLoginRequired, login } = useMcpOAuth();
  const visibleMcpServers = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) return mcpServers;
    return mcpServers.filter((server) =>
      `${server.name} ${server.description ?? ''}`.toLowerCase().includes(query)
    );
  }, [mcpServers, searchQuery]);
  const visibleExtensionServers = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) return extensionMcpServers;
    return extensionMcpServers.filter((server) =>
      `${server.name} ${server.description ?? ''} ${server.source_key}`.toLowerCase().includes(query)
    );
  }, [extensionMcpServers, searchQuery]);
  const hasServers = mcpServers.length > 0 || extensionMcpServers.length > 0;
  const hasVisibleServers = visibleMcpServers.length > 0 || visibleExtensionServers.length > 0;

  const handleAuthRequired = useCallback(
    (server: IMcpServer) => {
      markLoginRequired(server.mcp_server_id);
    },
    [markLoginRequired]
  );
  const handleAuthResolved = useCallback(
    (server: IMcpServer) => {
      clearLoginRequired(server.mcp_server_id);
    },
    [clearLoginRequired]
  );

  const { testingServers, handleTestMcpConnection, handleTestMcpConnections } = useMcpConnection(
    setMcpServers,
    handleAuthRequired,
    handleAuthResolved
  );
  const [togglingServers, setTogglingServers] = useState<Record<string, boolean>>({});

  const handleToggleEnabled = useCallback(
    async (server: IMcpServer) => {
      setTogglingServers((current) => ({ ...current, [server.mcp_server_id]: true }));
      try {
        await onToggleEnabled(server);
      } finally {
        setTogglingServers((current) => ({ ...current, [server.mcp_server_id]: false }));
      }
    },
    [onToggleEnabled]
  );

  useEffect(() => {
    testersRef.current = { handleTestMcpConnection, handleTestMcpConnections };
    return () => {
      testersRef.current = null;
    };
  }, [handleTestMcpConnection, handleTestMcpConnections, testersRef]);

  const handleOAuthLogin = useCallback(
    async (server: IMcpServer) => {
      const result = await login(server);

      if (result.success) {
        message.success(`${server.name}: ${t('settings.mcpOAuthLoginSuccess') || 'Login successful'}`);
        void handleTestMcpConnection(server);
      } else {
        message.error(`${server.name}: ${result.error || t('settings.mcpOAuthLoginFailed') || 'Login failed'}`);
      }
    },
    [login, message, t, handleTestMcpConnection]
  );

  useEffect(() => {
    if (isMcpServersLoading || mcpServersLoadFailed) return;
    const httpServers = mcpServers.filter(
      (s) => s.transport.type === 'http' || s.transport.type === 'sse' || s.transport.type === 'streamable_http'
    );
    if (httpServers.length > 0) {
      httpServers.forEach((server) => {
        void checkOAuthStatus(server);
      });
    }
  }, [checkOAuthStatus, isMcpServersLoading, mcpServers, mcpServersLoadFailed]);

  const loadErrorNotice = mcpServersLoadFailed ? (
    <div
      className='flex items-center justify-between gap-12px rd-12px border border-dashed border-arco-2 px-16px py-12px text-13px text-t-secondary'
      role='alert'
    >
      <span>{t('settings.mcpSyncError')}</span>
      {reloadMcpServers ? (
        <Button size='small' type='secondary' onClick={reloadMcpServers}>
          {t('common.retry')}
        </Button>
      ) : null}
    </div>
  ) : null;

  return (
    <div className='mx-auto flex w-full max-w-1180px flex-1 min-h-0'>
      {isMcpServersLoading ? (
        <SettingsContentLoading className='min-h-220px' />
      ) : mcpServersLoadFailed && !hasServers ? (
        <div
          className='flex min-h-180px flex-col items-center justify-center gap-10px rd-12px border border-dashed border-arco-2 px-24px py-24px text-center'
          role='alert'
        >
          <div className='text-14px text-t-secondary'>{t('settings.mcpSyncError')}</div>
          {reloadMcpServers ? (
            <Button size='small' type='secondary' onClick={reloadMcpServers}>
              {t('common.retry')}
            </Button>
          ) : null}
        </div>
      ) : (
        <div className='space-y-12px'>
          {loadErrorNotice}
          {hasVisibleServers ? (
            <>
              {visibleMcpServers.map((server) => {
                const uiKey = mcpServerUiKey(server.mcp_server_id);
                return (
                  <McpServerItem
                    key={server.mcp_server_id}
                    server={server}
                    isCollapsed={mcpCollapseKey[uiKey] || false}
                    isTestingConnection={testingServers[server.mcp_server_id] || false}
                    activationState={activationStates[server.mcp_server_id]}
                    activationError={activationErrors[server.mcp_server_id]}
                    oauthStatus={oauthStatus[server.mcp_server_id]}
                    isLoggingIn={loggingIn[server.mcp_server_id]}
                    isTogglingEnabled={togglingServers[server.mcp_server_id] || false}
                    onToggleCollapse={() => toggleServerCollapse(uiKey)}
                    onTestConnection={handleTestMcpConnection}
                    onEditServer={showEditMcpModal}
                    onDeleteServer={showDeleteConfirm}
                    onToggleEnabled={handleToggleEnabled}
                    onOAuthLogin={handleOAuthLogin}
                  />
                );
              })}
              {visibleExtensionServers.map((server) => {
                const uiKey = extensionMcpUiKey(server.source_key);
                return (
                  <ExtensionMcpServerItem
                    key={uiKey}
                    server={server}
                    isCollapsed={mcpCollapseKey[uiKey] || false}
                    onToggleCollapse={() => toggleServerCollapse(uiKey)}
                  />
                );
              })}
            </>
          ) : (
            <div className='py-24px text-center text-t-secondary text-14px border border-dashed border-border-2 rd-12px'>
              {t('settings.mcpNoServersFound')}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const ModalMcpManagementSection = React.forwardRef<
  McpInstalledPanelHandle,
  {
    message: MessageInstance;
    mcpServers: IMcpServer[];
    extensionMcpServers: ExtensionMcpServerContribution[];
    setMcpServers: React.Dispatch<React.SetStateAction<IMcpServer[]>>;
    saveMcpServers: (serversOrUpdater: IMcpServer[] | ((prev: IMcpServer[]) => IMcpServer[])) => Promise<void>;
    hideChrome?: boolean;
    searchQuery?: string;
    showList?: boolean;
    isMcpServersLoading?: boolean;
    mcpServersLoadFailed?: boolean;
    reloadMcpServers?: () => void;
    pendingActivation?: McpPendingActivation;
    pendingFocusIds?: string[];
    onPendingConsumed?: () => void;
  }
>(({
  message,
  mcpServers,
  extensionMcpServers,
  setMcpServers,
  saveMcpServers,
  hideChrome = false,
  searchQuery = '',
  showList = true,
  isMcpServersLoading = false,
  mcpServersLoadFailed = false,
  reloadMcpServers,
  pendingActivation,
  pendingFocusIds,
  onPendingConsumed,
}, ref) => {
  const { t } = useTranslation();
  const testersRef = useRef<McpConnectionTesters | null>(null);
  const {
    showMcpModal,
    editingMcpServer,
    deleteConfirmVisible,
    serverToDelete,
    mcpCollapseKey,
    showAddMcpModal,
    showEditMcpModal,
    hideMcpModal,
    showDeleteConfirm,
    hideDeleteConfirm,
    toggleServerCollapse,
  } = useMcpModal();
  const { handleAddMcpServer, handleBatchImportMcpServers, handleEditMcpServer, handleDeleteMcpServer, handleToggleMcpServer, handleActivateMcpServer } =
    useMcpServerCRUD(saveMcpServers);
  const [importMode, setImportMode] = useState<'json' | 'oneclick'>('json');

  const ensureServerExpanded = useCallback(
    (server: IMcpServer) => {
      const uiKey = mcpServerUiKey(server.mcp_server_id);
      if (!mcpCollapseKey[uiKey]) toggleServerCollapse(uiKey);
      requestAnimationFrame(() => {
        const element = document.querySelector<HTMLElement>(`[data-mcp-server-id="${server.mcp_server_id}"]`);
        element?.focus({ preventScroll: true });
        element?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      });
    },
    [mcpCollapseKey, toggleServerCollapse]
  );

  const {
    itemStates: activationStates,
    itemErrors: activationErrors,
    progress: activationProgress,
  } = useMcpActivationFlow({
    operation: pendingActivation,
    servers: mcpServers,
    isServersLoading: isMcpServersLoading,
    serversLoadFailed: mcpServersLoadFailed,
    activateServer: handleActivateMcpServer,
    ensureExpanded: ensureServerExpanded,
    onConsumed: onPendingConsumed,
  });

  // Import-only path: just expand (and scroll to) the imported rows once.
  const consumedFocusRef = useRef<string | null>(null);
  useEffect(() => {
    if (!pendingFocusIds || pendingActivation) return;
    if (isMcpServersLoading || mcpServersLoadFailed) return;
    const ids = new Set(pendingFocusIds);
    const focusKey = pendingFocusIds.join('|');
    if (consumedFocusRef.current === focusKey) return;
    const targets = mcpServers.filter((server) => ids.has(server.mcp_server_id));
    if (targets.length === 0) return;
    consumedFocusRef.current = focusKey;
    onPendingConsumed?.();
    targets.forEach(ensureServerExpanded);
  }, [ensureServerExpanded, isMcpServersLoading, mcpServers, mcpServersLoadFailed, onPendingConsumed, pendingActivation, pendingFocusIds]);

  useImperativeHandle(
    ref,
    () => ({
      openAdd: (mode) => {
        setImportMode(mode);
        showAddMcpModal();
      },
    }),
    [showAddMcpModal]
  );

  const wrappedHandleAddMcpServer = useCallback(
    async (serverData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>) => {
      const addedServer = await handleAddMcpServer(serverData);
      if (addedServer) {
        void testersRef.current?.handleTestMcpConnection(addedServer, { notify: false });
      }
    },
    [handleAddMcpServer]
  );

  const wrappedHandleEditMcpServer = useCallback(
    async (serverToEdit: IMcpServer | undefined, serverData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>) => {
      const updatedServer = await handleEditMcpServer(serverToEdit, serverData);
      if (updatedServer) {
        void testersRef.current?.handleTestMcpConnection(updatedServer, { notify: false });
      }
    },
    [handleEditMcpServer]
  );

  const wrappedHandleBatchImportMcpServers = useCallback(
    async (serversData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>[]) => {
      const addedServers = await handleBatchImportMcpServers(serversData);
      if (addedServers && addedServers.length > 0) {
        await testersRef.current?.handleTestMcpConnections(addedServers, { concurrency: 4, notify: false });
      }
      return addedServers;
    },
    [handleBatchImportMcpServers]
  );

  const handleConfirmDelete = useCallback(async () => {
    if (!serverToDelete) return;
    hideDeleteConfirm();
    await handleDeleteMcpServer(serverToDelete);
  }, [serverToDelete, hideDeleteConfirm, handleDeleteMcpServer]);

  return (
    <div className='flex flex-col gap-16px min-h-0'>
      {!hideChrome && (
      <div className='flex gap-8px items-center justify-between'>
        <div className='text-14px text-t-primary'>{t('settings.mcpSettings')}</div>
        <div>
          <McpAddChrome
            onOpenJson={() => {
              setImportMode('json');
              showAddMcpModal();
            }}
            onOpenOneClick={() => {
              setImportMode('oneclick');
              showAddMcpModal();
            }}
          />
        </div>
      </div>
      )}

      {showList ? (
        <>
          <McpActivationNotice progress={activationProgress} states={activationStates} servers={mcpServers} />
          <McpInstalledList
            message={message}
            mcpServers={mcpServers}
            extensionMcpServers={extensionMcpServers}
            setMcpServers={setMcpServers}
            searchQuery={searchQuery}
            mcpCollapseKey={mcpCollapseKey}
            toggleServerCollapse={toggleServerCollapse}
            showEditMcpModal={showEditMcpModal}
            showDeleteConfirm={showDeleteConfirm}
            onToggleEnabled={handleToggleMcpServer}
            testersRef={testersRef}
            isMcpServersLoading={isMcpServersLoading}
            mcpServersLoadFailed={mcpServersLoadFailed}
            reloadMcpServers={reloadMcpServers}
            activationStates={activationStates}
            activationErrors={activationErrors}
          />
        </>
      ) : null}

      <AddMcpServerModal
        visible={showMcpModal}
        server={editingMcpServer}
        existingServerNames={mcpServers.map((server) => server.name)}
        onCancel={hideMcpModal}
        onSubmit={
          editingMcpServer
            ? (serverData) => wrappedHandleEditMcpServer(editingMcpServer, serverData)
            : wrappedHandleAddMcpServer
        }
        onBatchImport={wrappedHandleBatchImportMcpServers}
        importMode={importMode}
      />

      <Modal
        title={t('settings.mcpDeleteServer')}
        visible={deleteConfirmVisible}
        onCancel={hideDeleteConfirm}
        onOk={handleConfirmDelete}
        okButtonProps={{ status: 'danger' }}
        okText={t('common.confirm')}
        cancelText={t('common.cancel')}
      >
        <p>{t('settings.mcpDeleteConfirm')}</p>
      </Modal>
    </div>
  );
});

ModalMcpManagementSection.displayName = 'ModalMcpManagementSection';

const ToolsModalContent: React.FC = () => {
  const [mcpMessage, mcpMessageContext] = useArcoMessage({ maxCount: 10 });
  const {
    mcpServers,
    extensionMcpServers,
    isUserMcpServersLoading,
    userMcpServersLoadFailed,
    reloadMcpServers,
    saveMcpServers,
    setMcpServers,
  } = useMcpServers();
  return (
    <ToolsModalContentWithState
      mcpMessage={mcpMessage}
      mcpMessageContext={mcpMessageContext}
      mcpServers={mcpServers}
      extensionMcpServers={extensionMcpServers}
      saveMcpServers={saveMcpServers}
      setMcpServers={setMcpServers}
      isMcpServersLoading={isUserMcpServersLoading}
      mcpServersLoadFailed={userMcpServersLoadFailed}
      reloadMcpServers={reloadMcpServers}
    />
  );
};

/**
 * State-injected variant so hosts that already own the MCP server state (e.g.
 * the /mcp hub page with its market tabs) can share one `useMcpServers`
 * instance across tabs instead of double-fetching.
 */
export const ToolsModalContentWithState = React.forwardRef<
  McpInstalledPanelHandle,
  {
    mcpMessage: MessageInstance;
    mcpMessageContext: React.ReactNode;
    mcpServers: IMcpServer[];
    extensionMcpServers: ExtensionMcpServerContribution[];
    setMcpServers: React.Dispatch<React.SetStateAction<IMcpServer[]>>;
    saveMcpServers: (serversOrUpdater: IMcpServer[] | ((prev: IMcpServer[]) => IMcpServer[])) => Promise<void>;
    hideChrome?: boolean;
    searchQuery?: string;
    showList?: boolean;
    isMcpServersLoading?: boolean;
    mcpServersLoadFailed?: boolean;
    reloadMcpServers?: () => void;
    pendingActivation?: McpPendingActivation;
    pendingFocusIds?: string[];
    onPendingConsumed?: () => void;
  }
>(({
  mcpMessage,
  mcpMessageContext,
  mcpServers,
  extensionMcpServers,
  saveMcpServers,
  setMcpServers,
  hideChrome,
  searchQuery,
  showList,
  isMcpServersLoading,
  mcpServersLoadFailed,
  reloadMcpServers,
  pendingActivation,
  pendingFocusIds,
  onPendingConsumed,
}, ref) => {
  const contentClassName =
    showList === false
      ? undefined
      : hideChrome
        ? 'px-16px py-20px md:px-24px'
        : 'flowy-settings-panel px-16px py-20px md:px-24px';

  return (
    <div className='flex flex-col h-full w-full'>
      {mcpMessageContext}

      <div className={contentClassName}>
        <ModalMcpManagementSection
          ref={ref}
          message={mcpMessage}
          mcpServers={mcpServers}
          extensionMcpServers={extensionMcpServers}
          setMcpServers={setMcpServers}
          saveMcpServers={saveMcpServers}
          hideChrome={hideChrome}
          searchQuery={searchQuery}
          showList={showList}
          isMcpServersLoading={isMcpServersLoading}
          mcpServersLoadFailed={mcpServersLoadFailed}
          reloadMcpServers={reloadMcpServers}
          pendingActivation={pendingActivation}
          pendingFocusIds={pendingFocusIds}
          onPendingConsumed={onPendingConsumed}
        />
      </div>
    </div>
  );
});

ToolsModalContentWithState.displayName = 'ToolsModalContentWithState';

export default ToolsModalContent;
