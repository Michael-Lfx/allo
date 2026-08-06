/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { resolveHealModel } from './healConversationModel';

const getAvailable = (p: any) => (p.models ?? []) as string[];
const PROVIDER_A = '0190f5fe-7c00-7a00-8000-00000000000a';
const PROVIDER_B = '0190f5fe-7c00-7a00-8000-00000000000b';
const PROVIDER_DEAD = '0190f5fe-7c00-7a00-8000-00000000dead';
const provs = [
  { id: PROVIDER_A, models: ['m1', 'm2'] },
  { id: PROVIDER_B, models: ['m3'] },
] as any[];

describe('resolveHealModel', () => {
  test('returns null when bound provider still available', () => {
    expect(resolveHealModel({ id: PROVIDER_A, use_model: 'm1' } as any, provs, getAvailable, undefined)).toBeNull();
  });
  test('heals to saved default when bound provider gone', () => {
    const r = resolveHealModel(
      { id: PROVIDER_DEAD, use_model: 'x' } as any,
      provs,
      getAvailable,
      { provider_id: PROVIDER_B, model: 'm3' } as any,
    );
    expect(r?.provider.id).toBe(PROVIDER_B);
    expect(r?.use_model).toBe('m3');
    expect(r?.reason).toBe('stale');
  });
  test('does not choose a model when no persisted default exists', () => {
    const r = resolveHealModel({ id: PROVIDER_DEAD, use_model: 'x' } as any, provs, getAvailable, undefined);
    expect(r).toBeNull();
  });
  test('returns null when there are no providers at all', () => {
    expect(resolveHealModel({ id: PROVIDER_DEAD, use_model: 'x' } as any, [], getAvailable, undefined)).toBeNull();
  });
  test('does not choose a model when an unbound conversation has no default', () => {
    const empty = resolveHealModel({ id: '', use_model: '' } as any, provs, getAvailable, undefined);
    expect(empty).toBeNull();

    const missing = resolveHealModel(undefined, provs, getAvailable, undefined);
    expect(missing).toBeNull();
  });
  test('defaults to saved default when unbound and a valid preference exists', () => {
    const r = resolveHealModel(undefined, provs, getAvailable, {
      provider_id: PROVIDER_B,
      model: 'm3',
    } as any);
    expect(r?.provider.id).toBe(PROVIDER_B);
    expect(r?.use_model).toBe('m3');
    expect(r?.reason).toBe('default');
  });
  test('does not fall back when saved default model is unavailable', () => {
    // saved default provider exists but its stored model is no longer offered
    const r = resolveHealModel(
      { id: PROVIDER_DEAD, use_model: 'x' } as any,
      provs,
      getAvailable,
      { provider_id: PROVIDER_A, model: 'zzz' } as any,
    );
    expect(r).toBeNull();
  });
});
