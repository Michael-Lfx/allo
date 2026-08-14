/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('model provider duplicate action', () => {
  test('hides every model-list add-model entry point', () => {
    const source = readSource(new URL('./ModelModalContent.tsx', import.meta.url));
    const headerStart = source.indexOf('{/* Header */}');
    const contentStart = source.indexOf('{/* Content Area */}');
    const header = source.slice(headerStart, contentStart);

    expect(headerStart).toBeGreaterThanOrEqual(0);
    expect(contentStart).toBeGreaterThan(headerStart);
    expect(header.includes("t('settings.addModel')")).toBe(false);
    expect(source.includes('AddModelModal')).toBe(false);
    expect(source.includes('addModelModalCtrl')).toBe(false);
    expect(source.includes("icon={<Plus")).toBe(false);
  });

  test('task-scoped model selectors do not reintroduce an add-model menu item', () => {
    const selectorSources = [
      new URL('../../../../pages/guid/components/GuidModelSelector.tsx', import.meta.url),
      new URL('../../../agent/TaskModelSelect.tsx', import.meta.url),
      new URL('../../../../pages/knowledge/KnowledgeModelSelector.tsx', import.meta.url),
    ].map(readSource);

    for (const source of selectorSources) {
      expect(source).not.toContain("key='add-model'");
      expect(source).not.toContain('settings.addModel');
      expect(source).not.toContain("add-model'");
    }
  });

  test('exposes a row action that clones the provider server-side', () => {
    const source = readSource(new URL('./ModelModalContent.tsx', import.meta.url));

    // Server-side clone endpoint replaces the old client-side config copy.
    expect(source.includes('ipcBridge.mode.cloneProvider')).toBe(true);
    expect(source.includes('cloneProviderConfig')).toBe(false);
    expect(source.includes('providerClone')).toBe(false);
    expect(source.includes('duplicatePlatform')).toBe(true);
    expect(source.includes('settings.copyProviderConfig')).toBe(true);
    expect(source.includes('<Copy theme')).toBe(true);
    expect(source.indexOf('icon={<Write size')).toBeLessThan(source.indexOf('<Copy theme'));
  });

  test('sends a localized copy name with the clone request', () => {
    const source = readSource(new URL('./ModelModalContent.tsx', import.meta.url));

    // The clone body carries "<source name> <localized suffix>" so the copy is
    // named in the user's language instead of the backend default.
    expect(source.includes("t('settings.providerCopySuffix'")).toBe(true);
    expect(/name: `\$\{platform\.name\} \$\{t\('settings\.providerCopySuffix'/.test(source)).toBe(true);
  });
});
