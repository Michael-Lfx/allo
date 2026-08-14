/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('TurnProcessDisclosure empty process items', () => {
  test('renders static label instead of a disabled toggle when there are no process items', () => {
    const source = readSource(new URL('./TurnProcessDisclosure.tsx', import.meta.url));

    expect(source.includes('<TaskGroup')).toBe(true);
    expect(source.includes('expandable={hasProcessItems}')).toBe(true);
    expect(source.includes('resolveTaskGroupStatus')).toBe(true);
  });
});

describe('TurnProcessDisclosure stream handoff', () => {
  test('leaves space after the process log before the answer body', () => {
    const cssSource = readSource(new URL('../messages.css', import.meta.url));
    expect(cssSource.includes('.turn-process-disclosure {')).toBe(true);
    expect(cssSource.includes('padding-bottom: 16px')).toBe(true);
    expect(cssSource.includes('.turn-process-disclosure [data-testid=\'beautiful-ui-tool-chip\']')).toBe(true);
    expect(cssSource.includes('.turn-process-disclosure [data-testid=\'beautiful-ui-tool-chip\']:hover')).toBe(true);
    expect(cssSource.includes('font-size: var(--conversation-message-font-size, 14px)')).toBe(true);
    expect(cssSource.includes('line-height: var(--conversation-message-line-height, 22px)')).toBe(true);
  });
});
