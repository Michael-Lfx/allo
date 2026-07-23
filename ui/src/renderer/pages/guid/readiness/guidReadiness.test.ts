/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  buildGuidStatusChips,
  buildGuidTaskReceipt,
  intentsForWorkspace,
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

  test('builds a receipt with execution plan steps', () => {
    const receipt = buildGuidTaskReceipt('fix-code', '修测试', 'C:/repo');
    expect(receipt?.goal).toBe('修测试');
    expect(receipt?.context).toBe('workspace');
    expect(receipt?.planSteps.length).toBeGreaterThanOrEqual(3);
  });

  test('status chips mark blocked project when workspace is required', () => {
    const readiness = resolveGuidReadiness({
      intentId: 'fix-code',
      hasModel: true,
      needsModelForAgent: true,
      workspaceDir: '',
    });
    const chips = buildGuidStatusChips({ readiness, hasDraft: true });
    expect(chips.find((chip) => chip.id === 'project')?.state).toBe('blocked');
    expect(chips.find((chip) => chip.id === 'plan')?.state).toBe('ready');
  });

  test('intents adapt to workspace presence', () => {
    const withWorkspace = intentsForWorkspace(true).map((item) => item.id);
    const withoutWorkspace = intentsForWorkspace(false).map((item) => item.id);
    expect(withWorkspace.includes('fix-code')).toBe(true);
    expect(withoutWorkspace.includes('research')).toBe(true);
    expect(withoutWorkspace.includes('fix-code')).toBe(false);
  });
});
