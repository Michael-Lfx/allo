/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('Presentation capability surfaces', () => {
  test('settings page follows Header -> Section -> List -> Row and avoids Card/Modal', () => {
    const source = read('./PresentationSettings.tsx');
    expect(source).toContain("layout='form'");
    expect(source).toContain('SettingsPageHeader');
    expect(source).toContain('SettingsGroup');
    expect(source).toContain('SettingsList');
    expect(source).toContain('SettingsRow');
    expect(source).not.toContain('<Card');
    expect(source).not.toContain('<Modal');
    // Destructive action is confirmed inline.
    expect(source).toContain('<Popconfirm');
    expect(source).toContain('usePresentationCapability');
  });

  test('settings navigation, route and prefetch register /settings/presentation', () => {
    const nav = read('./components/settingsNavigation.ts');
    expect(nav).toContain("id: 'presentation'");
    expect(nav).toContain("groupId: 'capabilities'");
    expect(nav).toContain("labelKey: 'presentation.settings.nav'");
    for (const shell of ['./components/SettingsSider.tsx', './components/SettingsPageWrapper.tsx']) {
      expect(read(shell)).toContain('presentation: Ppt');
    }
    expect(read('../../components/layout/Router.tsx')).toContain("path='/settings/presentation'");
    expect(read('./prefetch.ts')).toContain("import('./PresentationSettings')");
  });

  test('progress track animates only transform: scaleX on a 2px hairline', () => {
    const source = read('../../components/presentation/PresentationInstallProgress.tsx');
    expect(source).toContain('h-2px');
    expect(source).toContain('scaleX');
    expect(source).toContain('prefers-reduced-motion');
    expect(source).not.toMatch(/width:\s*`?\$\{/);
  });

  test('conversation notice is mounted in Nomi chat and subscribes to the realtime status', () => {
    expect(read('../conversation/platforms/nomi/NomiChat.tsx')).toContain('<PresentationInstallNotice />');
    const hook = read('../../hooks/presentation/usePresentationCapability.ts');
    expect(hook).toContain('ipcBridge.presentation.onStatus.on');
    expect(read('../../../common/adapter/ipcBridge.ts')).toContain("wsEmitter<IPresentationCapability>('presentationCapability.status')");
  });

  test('home slides mode lives in the bottom toolbar, not above the input', () => {
    const page = read('../guid/GuidPage.tsx');
    expect(page).toContain("id: 'system:ppt'");
    expect(page).toContain('PRESENTATION_SKILL_ID');
    expect(page).toContain("slidesActive={supportsHomeGoalCommand && homeCapability === 'slides'}");
    // Closing the create-presentation panel exits slides mode; the panel
    // itself closes because slide options are cleared.
    expect(page).toContain('onClose={closeSlidePanelToDirectory}');
    expect(page).toContain('<SlideOptionsControls');
    expect(page).not.toContain('entryStrip=');
    expect(page).not.toContain('SlideOptionsStrip');
    // The composer keeps the user's text; the option line is added at send time.
    expect(page).toContain('input: resolvedSendInput,');
    expect(page).toContain('initialSkillIds: sendSkillIds,');

    const row = read('../guid/components/GuidActionRow.tsx');
    expect(row).toContain('{slidesActive && slideControls}');
    const selector = read('../../components/agent/TaskProfileSelector.tsx');
    expect(selector).toContain("data-testid='task-profile-option-slides'");
    // Slides is a home-only mode; the persisted profile type stays office | coding.
    expect(read('../../stores/taskProfileStore.ts')).toContain("export type TaskProfile = 'office' | 'coding';");
  });
});
