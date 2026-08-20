/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SystemSettings.tsx', import.meta.url), 'utf8');

describe('SystemSettings load contract', () => {
  test('lazy-loads each settings panel so /settings/system does not parse sibling pages', () => {
    expect(source).toContain("React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/SystemModalContent'))");
    expect(source).toContain("React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/AboutModalContent'))");
    expect(source).toContain("React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/BrowserUseSettingsContent'))");
    expect(source).toContain("React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/ComputerUseSettingsContent'))");
    expect(source).not.toContain("import SystemModalContent from");
    expect(source).not.toContain("import AboutModalContent from");
    expect(source).not.toContain("import BrowserUseSettingsContent from");
    expect(source).not.toContain("import ComputerUseSettingsContent from");
  });

  test('keeps page chrome mounted while the selected panel chunk loads', () => {
    expect(source).toContain('<Suspense');
    expect(source).toContain('<SettingsPageWrapper');
  });
});
