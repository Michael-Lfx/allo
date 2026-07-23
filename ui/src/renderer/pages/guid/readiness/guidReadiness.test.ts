/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  buildGuidTaskReceipt,
  resolveGuidReadiness,
} from './guidReadiness';

describe('guidReadiness', () => {
  test('blocks send when nomi needs a model', () => {
    const result = resolveGuidReadiness({
      intentId: 'automate',
      hasModel: false,
      needsModelForAgent: true,
      workspaceDir: '',
    });
    expect(result.ready).toBe(false);
    expect(result.blocker).toBe('model');
    expect(result.primaryAction).toBe('addModel');
  });

  test('requires workspace for fix-code intent', () => {
    const result = resolveGuidReadiness({
      intentId: 'fix-code',
      hasModel: true,
      needsModelForAgent: true,
      workspaceDir: '',
    });
    expect(result.ready).toBe(false);
    expect(result.blocker).toBe('workspace');
    expect(result.primaryAction).toBe('linkWorkspace');
  });

  test('is ready when model and required workspace are present', () => {
    const result = resolveGuidReadiness({
      intentId: 'summarize',
      hasModel: true,
      needsModelForAgent: true,
      workspaceDir: 'C:/project',
    });
    expect(result.ready).toBe(true);
    expect(result.blocker).toBeNull();
    expect(result.primaryAction).toBe('send');
  });

  test('builds a receipt for selected intent', () => {
    const receipt = buildGuidTaskReceipt('fix-code', '修测试', 'C:/repo');
    expect(receipt?.goal).toBe('修测试');
    expect(receipt?.context).toBe('workspace');
  });
});
