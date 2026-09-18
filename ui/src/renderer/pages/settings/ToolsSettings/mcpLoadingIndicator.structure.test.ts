import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) => readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('MCP loading presentation contracts', () => {
  test('uses a CSS ring with reduced-motion support instead of a static icon', () => {
    const component = read('./McpLoadingIndicator.tsx');
    const styles = read('./McpLoadingIndicator.module.css');

    expect(component).toContain("size?: 'small' | 'medium'");
    expect(component).toContain('styles.ring');
    expect(styles).toContain('@keyframes mcp-loading-spin');
    expect(styles).toContain('transform: rotate(360deg)');
    expect(styles).toContain('prefers-reduced-motion: reduce');
    expect(styles).toContain('animation: none');
  });

  test('keeps an accessible loading action slot in the installed card', () => {
    const header = read('./McpServerHeader.tsx');
    const item = read('./McpServerItem.tsx');
    const details = read('./McpServerDetails.tsx');
    const tools = read('./McpServerToolsList.tsx');

    expect(header).toContain('McpLoadingIndicator');
    expect(header).not.toContain('LoadingOne');
    expect(header).toContain("disabled\n        className='flowy-icon-text-btn !min-w-150px justify-center'");
    expect(header).toContain("aria-busy='true'");
    expect(header).not.toContain("text-orange-500 text-16px font-bold leading-none'>△");
    expect(item).toContain('aria-busy=');
    expect(details).toContain('activationState?: McpActivationItemState');
    expect(tools).toContain('settings.mcpToolsLoading');
    expect(tools).toContain('settings.mcpToolsBeforeCheck');
    expect(tools).toContain('settings.mcpToolsEmpty');
    expect(tools).toContain('settings.mcpToolsUnavailable');
  });
});
