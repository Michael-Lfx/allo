/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const drawerSource = readFileSync(new URL('./PresetEditDrawer.tsx', import.meta.url), 'utf8');

describe('PresetEditDrawer prompt preview typography', () => {
  test('removes skill metadata before rendering the compact Markdown preview', () => {
    expect(drawerSource.includes("import { stripSkillFrontmatter }")).toBe(true);
    expect(
      drawerSource.includes(
        'const promptPreviewContent = useMemo(() => stripSkillFrontmatter(editContext).trim(), [editContext]);'
      )
    ).toBe(true);
    expect(/from ['"]@\/renderer\/components\/Markdown['"]/.test(drawerSource)).toBe(false);
    expect(drawerSource.includes("from '@/renderer/components/Markdown/LazyMarkdownView'")).toBe(true);
    expect(drawerSource.includes('<LazyMarkdownView hiddenCodeCopyButton compact>')).toBe(true);
    expect(
      drawerSource.match(
        /<LazyMarkdownView[^>]*compact[^>]*>[\s\S]*?\{promptPreviewContent\}[\s\S]*?<\/LazyMarkdownView>/
      )
    ).not.toBeNull();
    expect(drawerSource.includes('{editContext}</LazyMarkdownView>')).toBe(false);
    expect(drawerSource.includes('{editContext}</MarkdownView>')).toBe(false);
  });
});
