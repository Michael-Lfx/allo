import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { IMcpServer } from '@/common/config/storage';
import type { McpServerId } from '@/common/types/ids';

export type McpActivationNavigationState = {
  operationId: string;
  serverIds: McpServerId[];
  mode: 'add-and-enable';
  source: 'market';
  createdAt: number;
};

export type McpActivationItemState =
  | 'queued'
  | 'testing'
  | 'enabled'
  | 'failed'
  | 'needs-auth'
  | 'config-changed';

export type McpActivationItemStates = Record<string, McpActivationItemState>;

export type McpActivationProgress = {
  operationId: string;
  total: number;
  completed: number;
  currentServerId?: McpServerId;
};

type ActivationResult = {
  server: IMcpServer;
  enabled: boolean;
  configChanged: boolean;
  rejectionReason?: string;
  needsAuth: boolean;
};

type UseMcpActivationFlowOptions = {
  operation?: McpActivationNavigationState;
  servers: IMcpServer[];
  isServersLoading: boolean;
  serversLoadFailed: boolean;
  activateServer: (
    serverId: McpServerId,
    options?: { notify?: boolean }
  ) => Promise<ActivationResult | undefined>;
  ensureExpanded: (server: IMcpServer) => void;
  onConsumed?: () => void;
};

const EMPTY_STATES: McpActivationItemStates = {};

/**
 * Owns the one-shot market-to-installed activation hand-off. The operation is
 * consumed only after the user-server catalog is ready and every known target
 * has a card, so a slow catalog response cannot silently drop activation.
 */
export const useMcpActivationFlow = ({
  operation,
  servers,
  isServersLoading,
  serversLoadFailed,
  activateServer,
  ensureExpanded,
  onConsumed,
}: UseMcpActivationFlowOptions) => {
  const { t } = useTranslation();
  const [itemStates, setItemStates] = useState<McpActivationItemStates>(EMPTY_STATES);
  const [itemErrors, setItemErrors] = useState<Record<string, string>>({});
  const [progress, setProgress] = useState<McpActivationProgress | undefined>();
  const startedOperationRef = useRef<string | null>(null);
  const consumedOperationRef = useRef<string | null>(null);

  useEffect(() => {
    if (!operation) return;

    if (startedOperationRef.current === operation.operationId) return;

    setItemStates(
      Object.fromEntries(operation.serverIds.map((serverId) => [serverId, 'queued' as McpActivationItemState]))
    );
    setItemErrors({});
    setProgress({
      operationId: operation.operationId,
      total: operation.serverIds.length,
      completed: 0,
    });
  }, [operation]);

  const runOperation = useCallback(async () => {
    if (!operation || startedOperationRef.current === operation.operationId) return;
    if (isServersLoading || serversLoadFailed) return;

    const serversById = new Map(servers.map((server) => [server.mcp_server_id, server]));
    const targets = operation.serverIds
      .map((serverId) => serversById.get(serverId))
      .filter((server): server is IMcpServer => Boolean(server));

    startedOperationRef.current = operation.operationId;
    consumedOperationRef.current = operation.operationId;
    onConsumed?.();

    const missingIds = operation.serverIds.filter((serverId) => !serversById.has(serverId));
    if (missingIds.length > 0) {
      setItemStates((current) => {
        const next = { ...current };
        missingIds.forEach((serverId) => {
          next[serverId] = 'failed';
        });
        return next;
      });
      setItemErrors((current) => {
        const next = { ...current };
        missingIds.forEach((serverId) => {
          next[serverId] = t('settings.mcpActivationMissingServer');
        });
        return next;
      });
    }

    let completed = missingIds.length;
    setProgress({
      operationId: operation.operationId,
      total: operation.serverIds.length,
      completed,
    });

    for (const server of targets) {
      ensureExpanded(server);
      setProgress({
        operationId: operation.operationId,
        total: operation.serverIds.length,
        completed,
        currentServerId: server.mcp_server_id,
      });
      setItemStates((current) => ({ ...current, [server.mcp_server_id]: 'testing' }));

      let result: ActivationResult | undefined;
      try {
        result = await activateServer(server.mcp_server_id, { notify: false });
      } catch {
        result = undefined;
      }

      const nextState: McpActivationItemState = result?.configChanged
        ? 'config-changed'
        : result?.needsAuth
          ? 'needs-auth'
          : result?.enabled
            ? 'enabled'
            : 'failed';
      setItemStates((current) => ({ ...current, [server.mcp_server_id]: nextState }));
      if (nextState === 'failed' || nextState === 'config-changed') {
        setItemErrors((current) => ({
          ...current,
          [server.mcp_server_id]:
            result?.rejectionReason ||
            (nextState === 'config-changed'
              ? t('settings.mcpConfigChangedDuringTest')
              : t('settings.mcpActivationTestFailed')),
        }));
      }
      completed += 1;
      setProgress({
        operationId: operation.operationId,
        total: operation.serverIds.length,
        completed,
      });
    }
  }, [activateServer, ensureExpanded, isServersLoading, onConsumed, operation, servers, serversLoadFailed, t]);

  useEffect(() => {
    void runOperation();
  }, [runOperation]);

  const hasOperation = Boolean(operation) || Boolean(progress);
  const isRunning = Boolean(progress && progress.completed < progress.total);

  return {
    itemStates,
    itemErrors,
    progress,
    hasOperation,
    isRunning,
    consumedOperationId: consumedOperationRef.current,
  };
};
