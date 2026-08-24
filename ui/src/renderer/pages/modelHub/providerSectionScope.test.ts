/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const provider = readFileSync(
  new URL('../../components/settings/SettingsModal/contents/ModelModalContent.tsx', import.meta.url),
  'utf8'
);

describe('providers & keys section scope', () => {
  test('states where find-a-model-by-purpose moved to', () => {
    expect(provider.includes("t('settings.modelHub.provider.scopeNote')")).toBe(true);
  });

  test('keeps its actual job: provider/model list and credential editors', () => {
    expect(provider.includes('SortableProviderCard')).toBe(true);
    expect(provider.includes('SortableModelRow')).toBe(true);
    expect(provider.includes('AddPlatformModal')).toBe(true);
    expect(provider.includes('ProviderConnectionsSection')).toBe(true);
    expect(provider.includes('ModelModalityEditor')).toBe(true);
  });
});
