/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  buildFirstWinOutcomeSnapshot,
  shouldShowFirstWinOutcomeCard,
} from './firstWinOutcomeModel';

describe('firstWinOutcomeModel', () => {
  test('builds snapshot from assistant answer and file summary', () => {
    const snapshot = buildFirstWinOutcomeSnapshot([
      {
        type: 'text',
        position: 'right',
        content: { content: 'fix the failing test' },
      },
      {
        type: 'file_summary',
        diffs: [
          {
            file_name: 'app.ts',
            fullPath: 'src/app.ts',
            insertions: 3,
            deletions: 1,
          },
        ],
      },
      {
        type: 'text',
        position: 'left',
        content: { content: 'Root cause was a null guard. Tests are green now.' },
      },
    ]);

    expect(snapshot).not.toBeNull();
    expect(snapshot?.status).toBe('with_changes');
    expect(snapshot?.files[0]?.name).toBe('app.ts');
    expect(snapshot?.summary).toContain('Root cause');
    expect(
      shouldShowFirstWinOutcomeCard({
        isFirstWin: true,
        isProcessing: false,
        snapshot,
        dismissed: false,
      })
    ).toBe(true);
  });

  test('hides while processing or after first-win completes', () => {
    const snapshot = buildFirstWinOutcomeSnapshot([
      {
        type: 'text',
        position: 'left',
        content: { content: 'Done.' },
      },
    ]);
    expect(
      shouldShowFirstWinOutcomeCard({
        isFirstWin: true,
        isProcessing: true,
        snapshot,
        dismissed: false,
      })
    ).toBe(false);
    expect(
      shouldShowFirstWinOutcomeCard({
        isFirstWin: false,
        isProcessing: false,
        snapshot,
        dismissed: false,
      })
    ).toBe(false);
  });
});
