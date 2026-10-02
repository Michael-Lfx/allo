/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { beforeEach, describe, expect, test } from 'bun:test';

import { normalizeTaskProfile, useTaskProfileStore } from './taskProfileStore';

beforeEach(() => {
  useTaskProfileStore.setState({ taskProfile: 'office' });
});

describe('taskProfileStore', () => {
  test('defaults to office', () => {
    expect(useTaskProfileStore.getState().taskProfile).toBe('office');
  });

  test('setTaskProfile selects coding', () => {
    useTaskProfileStore.getState().setTaskProfile('coding');
    expect(useTaskProfileStore.getState().taskProfile).toBe('coding');
  });

  test('normalizeTaskProfile only admits office/coding, else office', () => {
    expect(normalizeTaskProfile('coding')).toBe('coding');
    expect(normalizeTaskProfile('office')).toBe('office');
    expect(normalizeTaskProfile(undefined)).toBe('office');
    expect(normalizeTaskProfile('garbage')).toBe('office');
    expect(normalizeTaskProfile(42)).toBe('office');
  });

  test('setTaskProfile rejects an unknown value', () => {
    useTaskProfileStore.getState().setTaskProfile('nonsense' as unknown as 'coding');
    expect(useTaskProfileStore.getState().taskProfile).toBe('office');
  });
});
