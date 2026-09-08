import type { McpServerId } from '@/common/types/ids';
import { useCallback } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { mcpService } from '@/common/adapter/ipcBridge';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { IMcpServer } from '@/common/config/storage';
import { toBackendMcpPayload } from './catalog';

const mergeServerState = (persisted: IMcpServer, fallback?: Partial<IMcpServer>): IMcpServer => ({
  ...persisted,
  last_test_status: persisted.last_test_status ?? fallback?.last_test_status,
  tools: persisted.tools ?? fallback?.tools,
  last_connected: persisted.last_connected ?? fallback?.last_connected,
  original_json: persisted.original_json ?? fallback?.original_json,
});

const replaceUserServer = (servers: IMcpServer[], nextServer: IMcpServer) => {
  const remainingServers = servers.filter((server) => server.builtin === true || server.mcp_server_id !== nextServer.mcp_server_id);
  const insertIndex = remainingServers.findIndex((server) => server.builtin === true);

  if (insertIndex === -1) {
    return [...remainingServers, nextServer];
  }

  remainingServers.splice(insertIndex, 0, nextServer);
  return remainingServers;
};

const getMcpRequestErrorMessage = (error: unknown, fallback: string): string => {
  if (isBackendHttpError(error) && error.backendMessage.trim()) return error.backendMessage;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === 'string' && error.trim()) return error;
  return fallback;
};
export const useMcpServerCRUD = (
  saveMcpServers: (serversOrUpdater: IMcpServer[] | ((prev: IMcpServer[]) => IMcpServer[])) => Promise<void>
) => {
  const { t } = useTranslation();

  const handleAddMcpServer = useCallback(
    async (serverData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>) => {
      try {
        const persisted = await mcpService.createServer.invoke(toBackendMcpPayload(serverData));
        const nextServer = mergeServerState(persisted, serverData);
        await saveMcpServers((prevServers) => replaceUserServer(prevServers, nextServer));
        return nextServer;
      } catch (error) {
        Message.error(getMcpRequestErrorMessage(error, t('settings.mcpImportFailed')));
        return undefined;
      }
    },
    [saveMcpServers, t]
  );

  const handleBatchImportMcpServers = useCallback(
    async (serversData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>[]) => {
      try {
        const imported = await mcpService.importServers.invoke({
          servers: serversData.map((server) => toBackendMcpPayload(server)),
        });

        const finalServers: IMcpServer[] = imported.map((importedServer) => {
          const original = serversData.find((server) => server.name === importedServer.name);
          return mergeServerState(importedServer, original);
        });

        await saveMcpServers((prevServers) => {
          let nextServers = prevServers.filter((server) => server.builtin === true);
          const existingUserServers = prevServers.filter((server) => server.builtin !== true);

          for (const server of existingUserServers) {
            if (!finalServers.some((next) => next.mcp_server_id === server.mcp_server_id || next.name === server.name)) {
              nextServers = [...nextServers, server];
            }
          }

          for (const server of finalServers) {
            nextServers = replaceUserServer(nextServers, server);
          }

          return nextServers;
        });

        return finalServers;
      } catch (error) {
        Message.error(getMcpRequestErrorMessage(error, t('settings.mcpImportFailed')));
        return [];
      }
    },
    [saveMcpServers, t]
  );

  const handleEditMcpServer = useCallback(
    async (
      editingMcpServer: IMcpServer | undefined,
      serverData: Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'>
    ): Promise<IMcpServer | undefined> => {
      if (!editingMcpServer) {
        return undefined;
      }

      try {
        const persisted = await mcpService.updateServer.invoke({
          mcp_server_id: editingMcpServer.mcp_server_id,
          data: toBackendMcpPayload(serverData),
        });
        const configurationChanged =
          JSON.stringify(editingMcpServer.transport) !== JSON.stringify(serverData.transport) ||
          editingMcpServer.original_json !== serverData.original_json;
        const nextServer = mergeServerState(
          persisted,
          configurationChanged ? undefined : { ...editingMcpServer, ...serverData }
        );
        await saveMcpServers((prevServers) =>
          prevServers.map((server) => (server.mcp_server_id === editingMcpServer.mcp_server_id ? nextServer : server))
        );

        Message.success(t('settings.mcpImportSuccess'));
        return nextServer;
      } catch (error) {
        Message.error(getMcpRequestErrorMessage(error, t('settings.mcpImportFailed')));
        return undefined;
      }
    },
    [saveMcpServers, t]
  );

  const handleDeleteMcpServer = useCallback(
    async (serverId: McpServerId) => {
      await mcpService.deleteServer.invoke({ mcp_server_id: serverId });
      await saveMcpServers((prevServers) => prevServers.filter((server) => server.mcp_server_id !== serverId));
      Message.success(t('settings.mcpDeleted'));
    },
    [saveMcpServers, t]
  );

  const handleToggleMcpServer = useCallback(
    async (server: IMcpServer): Promise<IMcpServer | undefined> => {
      try {
        const persisted = await mcpService.toggleServer.invoke({ mcp_server_id: server.mcp_server_id });
        const nextServer = mergeServerState(persisted, server);
        await saveMcpServers((prevServers) => replaceUserServer(prevServers, nextServer));
        Message.success(t(nextServer.enabled ? 'settings.mcpEnabledSuccess' : 'settings.mcpDisabledSuccess'));
        return nextServer;
      } catch (error) {
        Message.error(getMcpRequestErrorMessage(error, t('settings.mcpToggleFailed')));
        return undefined;
      }
    },
    [saveMcpServers, t]
  );

  // Explicit "add and enable": tests the persisted config by ID and, on
  // success, enables the server server-side. Failure keeps it disabled.
  const handleActivateMcpServer = useCallback(
    async (
      serverId: McpServerId,
      options?: { notify?: boolean }
    ): Promise<
      | {
          server: IMcpServer;
          enabled: boolean;
          configChanged: boolean;
          rejectionReason?: string;
          needsAuth: boolean;
        }
      | undefined
    > => {
      const notify = options?.notify ?? true;
      try {
        const result = await mcpService.activateServer.invoke({ mcp_server_id: serverId });
        const nextServer = mergeServerState(result.server);
        await saveMcpServers((prevServers) => replaceUserServer(prevServers, nextServer));
        const needsAuth = result.needs_auth ?? result.test.needsAuth ?? result.test.needs_auth ?? false;
        if (result.enabled && notify) {
          const toolCount = nextServer.tools?.length ?? 0;
          Message.success(t('settings.mcpActivatedSuccess', { count: toolCount }));
        } else if (!result.enabled && notify && !result.config_changed) {
          Message.error(
            result.enable_rejected_reason || getMcpRequestErrorMessage(undefined, t('settings.mcpTestFailed'))
          );
        }
        return {
          server: nextServer,
          enabled: result.enabled,
          configChanged: result.config_changed ?? false,
          rejectionReason: result.enable_rejected_reason,
          needsAuth,
        };
      } catch (error) {
        if (notify) Message.error(getMcpRequestErrorMessage(error, t('settings.mcpTestFailed')));
        return undefined;
      }
    },
    [saveMcpServers, t]
  );

  return {
    handleAddMcpServer,
    handleBatchImportMcpServers,
    handleEditMcpServer,
    handleDeleteMcpServer,
    handleToggleMcpServer,
    handleActivateMcpServer,
  };
};
