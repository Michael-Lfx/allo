import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) => readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('MCP activation flow contracts', () => {
  const crud = read('../../../hooks/mcp/useMcpServerCRUD.ts');
  const bridge = read('../../../../common/adapter/ipcBridge.ts');
  const panel = read('../../../components/settings/SettingsModal/contents/ToolsModalContent.tsx');
  const header = read('./McpServerHeader.tsx');
  const hook = read('../../../hooks/mcp/useMcpActivationFlow.ts');

  test('activation calls the server-gated activate endpoint by ID only', () => {
    expect(bridge).toContain('/api/mcp/servers/${p.mcp_server_id}/activate');
    expect(bridge).toContain('/api/mcp/servers/${p.mcp_server_id}/test');
    expect(bridge).toContain('enable_rejected_reason');
    // The activate request body must not carry transport — the persisted row
    // is the only authority.
    expect(bridge).toContain("activateServer: withResponseMap(");
    expect(crud).toContain('mcpService.activateServer.invoke({ mcp_server_id: serverId })');
  });

  test('one-shot auto activation consumes navigation state exactly once', () => {
    expect(hook).toContain('startedOperationRef.current === operation.operationId');
    expect(hook).toContain('isServersLoading || serversLoadFailed');
    expect(hook).toContain('missingIds');
    expect(hook).toContain('await activateServer(server.mcp_server_id, { notify: false })');
    expect(panel).toContain('onPendingConsumed?.()');
    // Activation must go through the gated activate endpoint, not the toggle.
    expect(panel).not.toContain('await handleToggleMcpServer');
  });

  test('installed rows expose a text status CTA instead of icon-only affordances', () => {
    expect(header).toContain('StatusActionCta');
    expect(header).toContain('settings.mcpStatusCtaEnable');
    expect(header).toContain('settings.mcpStatusCtaRetry');
    expect(header).toContain('settings.mcpStatusCtaCheck');
    expect(header).toContain('settings.mcpActivationChecking');
    expect(header).toContain('settings.mcpEnabledAndSelectable');
  });

  test('focused rows carry a stable hook for scroll-into-view', () => {
    expect(read('./McpServerItem.tsx')).toContain('data-mcp-server-id={server.mcp_server_id}');
    expect(panel).toContain('scrollIntoView');
  });
});
