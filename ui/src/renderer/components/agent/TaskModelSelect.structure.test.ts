/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TaskModelSelect.tsx', import.meta.url), 'utf8');

describe('TaskModelSelect catalog load contract', () => {
  test('forwards autoRefreshCatalog into the shared task catalog hook', () => {
    expect(source).toContain('autoRefreshCatalog?: boolean');
    expect(source).toContain('useModelsForTask(task, requiredTraits, { autoRefreshCatalog })');
  });
});
