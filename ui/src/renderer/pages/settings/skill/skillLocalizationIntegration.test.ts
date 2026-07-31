/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('skill localization integration', () => {
  test('preset editor resolves every persisted skill source through the shared display helper', () => {
    const source = readSource(new URL('../PresetSettings/PresetEditDrawer.tsx', import.meta.url));

    expect(source.includes('const LocalizedSkillContent')).toBe(true);
    expect(source.includes('resolveSkillDisplay(skill, localeKey)')).toBe(true);
    expect(source.match(/<LocalizedSkillContent/g)?.length).toBe(4);
    expect(source.includes("t('settings.pending'")).toBe(true);
  });

  test('the home launcher labels catalog Skills by source without treating them as preset configuration', () => {
    const page = readSource(new URL('../../guid/GuidPage.tsx', import.meta.url));
    const drawer = readSource(new URL('../../guid/components/PresetPickerDrawer.tsx', import.meta.url));

    expect(page.includes('conversation.skills.sources.${skill.source}')).toBe(true);
    expect(page.includes('skillId: skill.skillId')).toBe(true);
    expect(drawer.includes('filterSkillsByTags')).toBe(false);
    expect(drawer.includes('DrawerSkillCard')).toBe(false);
    expect(drawer.includes('localeKey={localeKey}')).toBe(true);
  });
});
