import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import type { IMcpServer } from '@/common/config/storage';
import {
  attachMcpMarketOrigin,
  getMcpMarketOrigin,
  isMcpMarketItemInstalled,
} from './McpMarketSettings';

const marketItem = {
  id: 'skillhub_mcp:playwright',
  name: 'Playwright MCP',
};

const server = (name: string, original_json: string): IMcpServer =>
  ({ name, original_json }) as IMcpServer;
const marketSource = readFileSync(new URL('./McpMarketSettings.tsx', import.meta.url), 'utf8');

describe('MCP market installed state', () => {
  test('persists exact market provenance inside the server original JSON', () => {
    const original = JSON.stringify({ mcpServers: { browser: { command: 'npx' } } });
    const marked = attachMcpMarketOrigin(original, marketItem.id);
    const installed = server('browser', marked);

    expect(JSON.parse(marked).mcpServers.browser.command).toBe('npx');
    expect(getMcpMarketOrigin(installed)).toBe(marketItem.id);
    expect(isMcpMarketItemInstalled(marketItem, [installed])).toBe(true);
    expect(isMcpMarketItemInstalled(marketItem, [])).toBe(false);
  });

  test('recognizes legacy imports by server name or market slug', () => {
    expect(isMcpMarketItemInstalled(marketItem, [server('playwright', '{}')])).toBe(true);
    expect(isMcpMarketItemInstalled(marketItem, [server('another-server', '{}')])).toBe(false);
    expect(getMcpMarketOrigin(server('broken', '{'))).toBeNull();
  });

  test('uses an explicit import-review flow without generic install commands or auto-testing', () => {
    expect(marketSource).toContain('showInstallCommand={false}');
    expect(marketSource).toContain("settings.mcpMarket.importConfig");
    expect(marketSource).toContain('pendingMarketSource');
    expect(marketSource).toContain('confirmMissingFields');
    expect(marketSource).toContain('setPendingServers(servers)');
    expect(marketSource).not.toContain('handleTestMcp');
  });

  test('offers import-only and add-and-enable as separate explicit CTAs', () => {
    expect(marketSource).toContain("settings.mcpMarket.importOnly");
    expect(marketSource).toContain("settings.mcpMarket.addAndEnable");
    // The primary CTA carries the activation intent through navigation state;
    // the market page itself never runs a test or enables anything.
    expect(marketSource).toContain("handleConfirmImport('import')");
    expect(marketSource).toContain("handleConfirmImport('activate')");
    expect(marketSource).toContain("operationId: globalThis.crypto.randomUUID()");
    expect(marketSource).toContain("mode: 'add-and-enable'");
    expect(marketSource).toContain("source: 'market'");
    expect(marketSource).not.toContain('mcpAutoTest');
    expect(marketSource).toContain('mcpFocusIds');
  });
});
