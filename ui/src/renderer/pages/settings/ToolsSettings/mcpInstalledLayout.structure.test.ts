import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) => readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('MCP installed layout contracts', () => {
  test('edit modal keeps footer visible instead of a clipped 450px shell', () => {
    const modal = read('../components/JsonImportModal.tsx');

    expect(modal).toContain("maxHeight: '90vh'");
    expect(modal).toContain("maxHeight: 'calc(90vh - 160px)'");
    expect(modal).toContain("overflow: 'auto'");
    expect(modal).not.toContain('height: 450');
    expect(modal).not.toContain('height: 420 - 80');
  });

  test('installed rows use a compact card header with a single stateful CTA', () => {
    const collapse = read('./mcpServerCollapse.ts');
    const item = read('./McpServerItem.tsx');
    const extension = read('./ExtensionMcpServerItem.tsx');
    const header = read('./McpServerHeader.tsx');
    const details = read('./McpServerDetails.tsx');

    expect(collapse).toContain("[&_.arco-collapse-item-header-title]:!flex");
    expect(collapse).toContain("[&_.arco-collapse-item-header-title]:!items-center");
    expect(collapse).toContain('MCP_SERVER_TITLE_CLASS');
    expect(collapse).toContain('-translate-y-2px');
    expect(collapse).toContain('[&_.arco-collapse-item-header]:!pl-40px');
    expect(collapse).toContain('[&_.arco-collapse-item-header]:!py-14px');
    expect(collapse).toContain('[&_.arco-collapse-item-icon-hover]:!top-26px');
    expect(collapse).toContain('[&_.arco-collapse-item-content-box]:!pl-40px');
    expect(collapse).toContain('[&_.arco-collapse-item-content-box]:!pb-16px');
    expect(item).toContain('MCP_SERVER_COLLAPSE_CLASS');
    expect(extension).toContain('MCP_SERVER_COLLAPSE_CLASS');
    expect(header).toContain('MCP_SERVER_TITLE_CLASS');
    expect(extension).toContain('MCP_SERVER_TITLE_CLASS');
    expect(header).toContain('inline-flex h-24px w-24px shrink-0 items-center justify-center');
    expect(header).toContain('flowy-button-icon');
    expect(header).toContain('StatusActionCta');
    expect(header).toContain('settings.mcpRetestConnection');
    expect(header).toContain('settings.mcpDisable');
    expect(header).toContain('activationState');
    expect(header).not.toContain('<Switch');
    expect(header).not.toContain('disabled={!server.enabled && !canEnable}');
    expect(header).toContain('settings.mcpViewConfig');
    expect(header).not.toContain('invisible group-hover:visible');
    expect(header).not.toContain('h-[24px]');
    expect(extension).not.toContain('h-[24px]');
    expect(item).toContain('McpServerDetails');
    expect(details).toContain('settings.mcpAvailableToAgent');
    expect(details).toContain('settings.mcpUnavailableToAgent');
    expect(details).toContain('McpServerToolsList');
  });

  test('installed list keeps full width when rows are collapsed', () => {
    const tools = read('../../../components/settings/SettingsModal/contents/ToolsModalContent.tsx');

    expect(tools).toContain("'w-full min-w-0 space-y-12px'");
    expect(tools).toContain('mx-auto flex w-full max-w-1180px flex-1 min-h-0');
  });
});
