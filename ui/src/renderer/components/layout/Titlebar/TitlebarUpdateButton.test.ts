/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TitlebarUpdateButton.tsx', import.meta.url), 'utf8');

describe('TitlebarUpdateButton', () => {
  test('renders only when a desktop update is available', () => {
    expect(source.includes('if (!isDesktopShell() || !hasUpdate) return null;')).toBe(true);
    expect(source.includes("t('settings.checkForUpdates')")).toBe(false);
  });
});
