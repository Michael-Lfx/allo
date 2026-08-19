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

  test('installed rows center title, status icon, and actions on one midline', () => {
    const collapse = read('./mcpServerCollapse.ts');
    const item = read('./McpServerItem.tsx');
    const extension = read('./ExtensionMcpServerItem.tsx');
    const header = read('./McpServerHeader.tsx');

    expect(collapse).toContain("[&_.arco-collapse-item-header-title]:!flex");
    expect(collapse).toContain("[&_.arco-collapse-item-header-title]:!items-center");
    expect(collapse).toContain('MCP_SERVER_TITLE_CLASS');
    expect(collapse).toContain('-translate-y-2px');
    expect(item).toContain('MCP_SERVER_COLLAPSE_CLASS');
    expect(extension).toContain('MCP_SERVER_COLLAPSE_CLASS');
    expect(header).toContain('MCP_SERVER_TITLE_CLASS');
    expect(extension).toContain('MCP_SERVER_TITLE_CLASS');
    expect(header).toContain('inline-flex h-24px w-24px shrink-0 items-center justify-center');
    expect(header).toContain('flowy-button-icon');
    expect(header).not.toContain('h-[24px]');
    expect(extension).not.toContain('h-[24px]');
  });
});
