

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('capability hub navigation', () => {
  test('keeps presets, skills, and MCP out of the primary rail', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));
    const settingsNavigationSource = readSource(
      new URL('../../../pages/settings/components/settingsNavigation.ts', import.meta.url)
    );

    expect(siderSource.includes('<SiderPresetEntry')).toBe(false);
    expect(siderSource.includes('<SiderSkillsEntry')).toBe(false);
    expect(siderSource.includes('<SiderMcpEntry')).toBe(false);
    expect(siderSource.includes('<SiderConfigGroup')).toBe(false);
    expect(siderSource.includes('SiderExtensionsEntry')).toBe(false);

    expect(settingsNavigationSource.includes("id: 'capability-hub'")).toBe(true);
    expect(settingsNavigationSource.includes("id: 'presets'")).toBe(false);
    expect(settingsNavigationSource.includes("id: 'skills'")).toBe(false);
    expect(settingsNavigationSource.includes("id: 'mcp'")).toBe(false);
    expect(settingsNavigationSource.includes("id: 'plugins'")).toBe(false);
  });

  test('shows the full capability rail without first-win collapse', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('useFirstWinMode')).toBe(false);
    expect(siderSource.includes('showCapabilityHub')).toBe(false);
    expect(siderSource.includes('sider-more-capabilities')).toBe(false);
    expect(siderSource.includes('SiderLearningEntry')).toBe(true);
    expect(siderSource.includes('SiderKnowledgeEntry')).toBe(true);
    expect(siderSource.includes('SiderVideoGenerationGroup')).toBe(true);
    expect(siderSource.includes('SiderNomiEntry')).toBe(false);
    expect(siderSource.includes('sider-conversation-entry')).toBe(false);
  });

  test('places Learning directly below Knowledge with a development badge', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));
    const learningEntrySource = readSource(new URL('./SiderNav/SiderLearningEntry.tsx', import.meta.url));

    expect(siderSource.indexOf('<SiderKnowledgeEntry')).toBeLessThan(siderSource.indexOf('<SiderLearningEntry'));
    expect(learningEntrySource.includes("t('learning.dev.tag')")).toBe(true);
    expect(learningEntrySource.includes("t('learning.dev.navTooltip')")).toBe(true);
  });

  test('places Eval below Learning and gates it on developer mode', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));
    const evalEntrySource = readSource(new URL('./SiderNav/SiderEvalEntry.tsx', import.meta.url));

    expect(siderSource.includes('SiderEvalEntry')).toBe(true);
    expect(siderSource.includes("useConfig('system.developerMode')")).toBe(true);
    expect(siderSource.includes('developerMode === true')).toBe(true);
    expect(siderSource.indexOf('<SiderLearningEntry')).toBeLessThan(siderSource.indexOf('<SiderEvalEntry'));
    expect(evalEntrySource.includes("t('eval.dev.tag')")).toBe(true);
    expect(evalEntrySource.includes("t('eval.dev.navTooltip')")).toBe(true);
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
    expect(routerSource.includes("path='/plugins'")).toBe(true);
    expect(routerSource.includes('LegacyExtensionsRedirect')).toBe(true);
    expect(routerSource.includes("path='/extensions'")).toBe(true);
  });
});
