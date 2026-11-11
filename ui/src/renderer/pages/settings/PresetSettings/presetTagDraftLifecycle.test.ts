/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { createPresetTagDraftLifecycle } from './presetTagDraftLifecycle';

describe('preset tag draft lifecycle', () => {
  test('resets the tag draft before saving without changing drawer visibility', () => {
    const calls: string[] = [];
    const lifecycle = createPresetTagDraftLifecycle(
      { current: { resetPendingTag: () => calls.push('scenario') } },
      (visible) => calls.push(`visible:${visible}`),
      async () => {
        calls.push('save');
        return null;
      }
    );

    lifecycle.handleDrawerSave();

    expect(calls).toEqual(['scenario', 'save']);
  });

  test('resets the tag draft before closing the drawer', () => {
    const calls: string[] = [];
    const lifecycle = createPresetTagDraftLifecycle(
      { current: { resetPendingTag: () => calls.push('scenario') } },
      (visible) => calls.push(`visible:${visible}`),
      async () => {
        calls.push('save');
        return null;
      }
    );

    lifecycle.closeDrawer();

    expect(calls).toEqual(['scenario', 'visible:false']);
  });

  test('allows the drawer to be hidden before the picker mounts', () => {
    const lifecycle = createPresetTagDraftLifecycle(
      { current: null },
      () => {},
      async () => null
    );

    lifecycle.resetPendingTagDrafts();
  });
});
