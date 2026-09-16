/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const layoutSource = readFileSync(new URL('../../components/layout/Layout.tsx', import.meta.url), 'utf8');

describe('active conversation route sync wiring', () => {
  test('the persistent layout keeps the route-derived active conversation synced', () => {
    expect(layoutSource.includes("from '@renderer/hooks/ui/useActiveConversationRouteSync'")).toBe(true);
    expect(layoutSource.includes('useActiveConversationRouteSync();')).toBe(true);
  });
});
