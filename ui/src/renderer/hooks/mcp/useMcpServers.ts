import { useCallback, useEffect, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import type { IMcpServer } from '@/common/config/storage';
import { ensureBackendMcpCatalog } from './catalog';
import { parseExtensionMcpServers, type ExtensionMcpServerContribution } from './extensionCatalog';

/**
 * MCP server state hook.
 * Combines backend-managed user servers with extension-contributed servers.
 */
export const useMcpServers = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled ?? true;
  const [mcpServers, setMcpServers] = useState<IMcpServer[]>([]);
  const [extensionMcpServers, setExtensionMcpServers] = useState<ExtensionMcpServerContribution[]>([]);
  const [isMcpServersLoading, setIsMcpServersLoading] = useState(enabled);
  const [isExtensionMcpServersLoading, setIsExtensionMcpServersLoading] = useState(enabled);
  const [mcpServersLoadFailed, setMcpServersLoadFailed] = useState(false);
  const requestIdRef = useRef(0);

  const loadMcpServers = useCallback(() => {
    if (!enabled) return;

    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setIsMcpServersLoading(true);
    setIsExtensionMcpServersLoading(true);
    void ensureBackendMcpCatalog()
      .then(({ allServers }) => {
        if (requestIdRef.current !== requestId) return;
        setMcpServers(allServers);
        setMcpServersLoadFailed(false);
      })
      .catch((error) => {
        if (requestIdRef.current !== requestId) return;
        console.error('[useMcpServers] Failed to load MCP catalog:', error);
        setMcpServers([]);
        setMcpServersLoadFailed(true);
      })
      .finally(() => {
        if (requestIdRef.current === requestId) setIsMcpServersLoading(false);
      });

    void ipcBridge.extensions.getMcpServers
      .invoke()
      .then((extServers) => {
        if (requestIdRef.current !== requestId) return;
        if (!extServers || extServers.length === 0) {
          setExtensionMcpServers([]);
          return;
        }

        const converted = parseExtensionMcpServers(extServers);
        if (converted.length !== extServers.length) {
          console.warn(
            `[useMcpServers] Ignored ${extServers.length - converted.length} malformed extension MCP contribution(s)`
          );
        }
        setExtensionMcpServers(converted);
      })
      .catch((error) => {
        if (requestIdRef.current !== requestId) return;
        console.error('[useMcpServers] Failed to load extension MCP servers:', error);
        setExtensionMcpServers([]);
      })
      .finally(() => {
        if (requestIdRef.current === requestId) setIsExtensionMcpServersLoading(false);
      });
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      requestIdRef.current += 1;
      setIsMcpServersLoading(false);
      setIsExtensionMcpServersLoading(false);
      return;
    }

    loadMcpServers();
    return () => {
      requestIdRef.current += 1;
    };
  }, [enabled, loadMcpServers]);

  const saveMcpServers = useCallback((serversOrUpdater: IMcpServer[] | ((prev: IMcpServer[]) => IMcpServer[])) => {
    return new Promise<void>((resolve) => {
      setMcpServers((prevServers) => {
        const nextServers = typeof serversOrUpdater === 'function' ? serversOrUpdater(prevServers) : serversOrUpdater;
        queueMicrotask(resolve);
        return nextServers;
      });
    });
  }, []);

  return {
    mcpServers,
    isMcpServersLoading: isMcpServersLoading || isExtensionMcpServersLoading,
    mcpServersLoadFailed,
    reloadMcpServers: loadMcpServers,
    allMcpServers: [...mcpServers, ...extensionMcpServers],
    extensionMcpServers,
    setMcpServers,
    saveMcpServers,
  };
};
