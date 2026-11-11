/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('SkillCard interaction ownership', () => {
  test('opens details from the shared market tile and keeps tag editing on its own action', () => {
    const source = readFileSync(new URL('./SkillCard.tsx', import.meta.url), 'utf8');

    expect(source).toContain('<ExpertTile');
    expect(source).toContain('onOpen={() => onOpenDetails(skill)}');
    expect(source).toContain('onAction={() => onEditTags(skill)}');
    expect(source).not.toContain('onDelete');
    expect(source).not.toContain('btn-delete-');
    expect(source).toContain("t('settings.skillsHub.editTags'");
    expect(source).not.toContain('pb-42px');
    expect(source).not.toContain('bg-[var(--color-bg-2)]');
  });
});
