

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('capability hub navigation', () => {
  test('collapses presets, skills, and MCP under Config', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('SiderConfigGroup')).toBe(true);
    expect(siderSource.includes('siderSection.config')).toBe(true);
    expect(siderSource.includes('<SiderPresetEntry')).toBe(false);
    expect(siderSource.includes('<SiderSkillsEntry')).toBe(false);
    expect(siderSource.includes('<SiderMcpEntry')).toBe(false);
    expect(siderSource.includes('SiderExtensionsEntry')).toBe(false);
  });

  test('shows the full capability rail without first-win collapse', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('useFirstWinMode')).toBe(false);
    expect(siderSource.includes('showCapabilityHub')).toBe(false);
    expect(siderSource.includes('sider-more-capabilities')).toBe(false);
    expect(siderSource.includes('SiderLearningEntry')).toBe(true);
    expect(siderSource.includes('SiderKnowledgeEntry')).toBe(true);
    expect(siderSource.includes('SiderVideoGenerationEntry')).toBe(true);
    expect(siderSource.includes('sider-conversation-entry')).toBe(false);
  });

  test('places Learning directly below Knowledge with a development badge', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));
    const learningEntrySource = readSource(new URL('./SiderNav/SiderLearningEntry.tsx', import.meta.url));

    expect(siderSource.indexOf('<SiderKnowledgeEntry')).toBeLessThan(siderSource.indexOf('<SiderLearningEntry'));
    expect(learningEntrySource.includes("t('learning.dev.tag')")).toBe(true);
    expect(learningEntrySource.includes("t('learning.dev.navTooltip')")).toBe(true);
  });

  test('keeps Open Capabilities routes compatible while hiding its settings entry', () => {
    const routerSource = readSource(new URL('../Router.tsx', import.meta.url));
    const settingsSiderSource = readSource(
      new URL('../../../pages/settings/components/SettingsSider.tsx', import.meta.url)
    );

    expect(routerSource.includes("path='/open-capabilities'")).toBe(true);
    expect(routerSource.includes("path='/settings/open-capabilities'")).toBe(true);
    expect(settingsSiderSource.includes("'open-capabilities'")).toBe(false);
    expect(routerSource.includes("path='/settings/webui' element={<Navigate to='/open-capabilities'")).toBe(true);
    expect(routerSource.includes("path='/settings/tools' element={<Navigate to='/open-capabilities'")).toBe(true);
    expect(routerSource.includes('getHashRouteRedirectUrl')).toBe(true);
    expect(routerSource.includes("path='/mcp'")).toBe(true);
    expect(routerSource.includes("path='/presets'")).toBe(true);
    expect(routerSource.includes("path='/skills'")).toBe(true);
    expect(routerSource.includes('LegacyExtensionsRedirect')).toBe(true);
    expect(routerSource.includes("path='/extensions'")).toBe(true);
  });
});
