import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) =>
  readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('capability hub load contracts', () => {
  test('settings drawers do not statically import Markdown or the Preview barrel', () => {
    const drawer = read('./PresetSettings/PresetEditDrawer.tsx');
    const skillDrawer = read('./skill/SkillDetailDrawer.tsx');
    const mermaid = read('../../components/Markdown/MermaidBlock.tsx');
    const codeBlock = read('../../components/Markdown/CodeBlock.tsx');

    expect(drawer).toContain("from '@/renderer/components/Markdown/LazyMarkdownView'");
    expect(drawer).toContain('enabled: editVisible');
    expect(drawer).toContain('unmountOnExit');
    expect(skillDrawer).toContain('unmountOnExit');
    expect(/from ['"]@\/renderer\/components\/Markdown['"]/.test(drawer)).toBe(false);
    expect(skillDrawer).toContain("from '@/renderer/components/Markdown/LazyMarkdownView'");
    expect(/from ['"]@\/renderer\/components\/Markdown['"]/.test(skillDrawer)).toBe(false);
    expect(codeBlock).toContain("React.lazy(() => import('./MermaidBlock'))");
    expect(/from ['"]@\/renderer\/pages\/conversation\/Preview['"]/.test(mermaid)).toBe(false);
    expect(mermaid).toContain("from '@/renderer/pages/conversation/Preview/context/PreviewContext'");
  });

  test('MCP market keeps openAdd without mounting list OAuth or agent detection', () => {
    const tools = read('../../components/settings/SettingsModal/contents/ToolsModalContent.tsx');
    const listFn = tools.indexOf('function McpInstalledList');
    const oauth = tools.indexOf('useMcpOAuth()');
    const connection = tools.indexOf('useMcpConnection(');
    const getAgentsInAddChrome = tools.indexOf('function McpAddChrome');
    const getAgentsCall = tools.indexOf('getAgents()');

    expect(listFn).toBeGreaterThan(-1);
    expect(tools).toContain('openAdd:');
    expect(tools).toContain('<AddMcpServerModal');
    expect(tools).toContain('{showList ? (');
    expect(tools).toContain('<McpInstalledList');
    expect(oauth).toBeGreaterThan(listFn);
    expect(connection).toBeGreaterThan(listFn);
    expect(getAgentsInAddChrome).toBeGreaterThan(-1);
    expect(getAgentsCall).toBeGreaterThan(getAgentsInAddChrome);
    expect(getAgentsCall).toBeLessThan(listFn);
  });

  test('capability installed views distinguish loading from an empty result', () => {
    const presetList = read('./PresetSettings/PresetListPanel.tsx');
    const skills = read('./SkillsHubSettings.tsx');
    const plugins = read('../mcp/PluginSettingsPanel.tsx');
    const mcp = read('../../components/settings/SettingsModal/contents/ToolsModalContent.tsx');

    for (const source of [presetList, skills, plugins, mcp]) {
      expect(source).toContain('SettingsContentLoading');
    }
    expect(presetList).toContain('loading ?');
    expect(skills).toContain('{loading && availableSkills.length === 0 ? (');
    expect(skills).toContain('loadError && availableSkills.length === 0');
    expect(plugins).toContain('{loading ? (');
    expect(mcp).toContain('{isMcpServersLoading ? (');
    expect(mcp).toContain('mcpServersLoadFailed');
    expect(mcp).toContain("t('common.retry')");
  });

  test('MCP catalog refreshes expose one combined loading state and ignore stale responses', () => {
    const hook = read('../../hooks/mcp/useMcpServers.ts');

    expect(hook).toContain('requestIdRef');
    expect(hook).toContain('if (requestIdRef.current !== requestId) return;');
    expect(hook).toContain('isMcpServersLoading: isMcpServersLoading || isExtensionMcpServersLoading');
    expect(hook).toContain('reloadMcpServers: loadMcpServers');
  });

  test('skill market reuses the available-skills SWR key and does not scan home-dir agents', () => {
    const market = read('./SkillMarketSettings.tsx');
    const importMenu = read('./skill/SkillImportMenu.tsx');
    const panel = read('./MarketSettingsPanel.tsx');

    expect(market).toContain('AVAILABLE_SKILLS_SWR_KEY');
    expect(market).not.toContain('detectAndCountExternalSkills');
    expect(importMenu).toContain('openAgentImport');
    expect(importMenu).not.toContain('useEffect');
    expect(panel).toContain('usePresetTags({ enabled: enableTagFilter })');
  });

  test('capability hub chrome uses two-row discover and installed controls', () => {
    const header = read('./capabilityHub/CapabilityHubHeader.tsx');
    const shell = read('./capabilityHub/CapabilityHubShell.tsx');
    const styles = read('./components/settings.css');

    expect(header).toContain('capability-hub-toolbar-pair');
    expect(header).toContain('capability-hub-tabs-row');
    expect(header).toContain('capability-hub-segment');
    expect(header).toContain("data-testid='capability-hub-installed'");
    expect(header).toContain("data-testid='capability-hub-search'");
    expect(header).toContain("settings.capabilityHub.discover");
    expect(header).toContain("if (view === 'installed') onToggleInstalled()");
    expect(header).toContain("if (view === 'market') onToggleInstalled()");
    expect(header.indexOf('capability-hub-actions')).toBeLessThan(header.indexOf('capability-hub-toolbar-pair'));
    expect(shell).toContain("className='capability-hub-page'");
    expect(shell).not.toContain('md:!pt-20px');
    expect(styles).not.toContain('.capability-hub-page {\n  background:');
    expect(styles).toContain('grid-template-columns: 1fr 1fr');
    expect(styles).toContain('width: 2ch');
    expect(styles).toContain('font-variant-numeric: tabular-nums');
  });
});
