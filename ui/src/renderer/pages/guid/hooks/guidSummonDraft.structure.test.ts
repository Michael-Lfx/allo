/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Structure tests: Guid landing page no longer exposes a summon-companion
 * draft entry in the composer strip. In-session summon remains on NomiSendBox.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid summon draft integration', () => {
  test('GuidPage does not wire a summon companion composer entry', () => {
    const page = readSource(new URL('../GuidPage.tsx', import.meta.url));
    expect(page.includes('onSummonCompanion')).toBe(false);
    expect(page.includes('SummonDrawer')).toBe(false);
    expect(page.includes('guid-summon-entry')).toBe(false);
  });

  test('ComposerEntryStrip has no summon companion controls', () => {
    const strip = readSource(new URL('../components/ComposerEntryStrip.tsx', import.meta.url));
    expect(strip.includes('onSummonCompanion')).toBe(false);
    expect(strip.includes('guid-summon-entry')).toBe(false);
    expect(strip.includes('conversation.summon')).toBe(false);
  });

  test('advanced-config drafts no longer stage summon picks', () => {
    const source = readSource(new URL('./useGuidAdvancedConfig.ts', import.meta.url));
    expect(source.includes('setSummon')).toBe(false);
    expect(source.includes('ipcBridge.conversation.setSummon.invoke')).toBe(false);
  });
});
