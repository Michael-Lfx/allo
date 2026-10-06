/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  classifyOtpVerificationError,
  isEmail,
  sendEmailOtpCode,
} from './useEmailOtpLogin';

describe('email OTP send smoke', () => {
  test('rejects invalid emails before any transport call', async () => {
    let startCalls = 0;
    let continueCalls = 0;
    const result = await sendEmailOtpCode('not-an-email', null, {
      loginStart: async () => {
        startCalls += 1;
        return { pendingId: 'p1' };
      },
      loginContinue: async () => {
        continueCalls += 1;
        return { status: 'pending', pendingId: 'p1', method: 'email_otp', message: 'sent' };
      },
    });
    expect(result).toEqual({ ok: false, kind: 'invalid-email' });
    expect(startCalls).toBe(0);
    expect(continueCalls).toBe(0);
    expect(isEmail('user@example.com')).toBe(true);
    expect(isEmail('bad')).toBe(false);
  });

  test('starts a session then continues with the email address', async () => {
    const calls: Array<{ kind: string; payload?: unknown }> = [];
    const result = await sendEmailOtpCode('user@example.com', null, {
      loginStart: async () => {
        calls.push({ kind: 'start' });
        return { pendingId: 'pending-1' };
      },
      loginContinue: async (args) => {
        calls.push({ kind: 'continue', payload: args });
        return { status: 'pending', pendingId: 'pending-2', method: 'email_otp', message: 'sent' };
      },
    });
    expect(result).toEqual({ ok: true, pendingId: 'pending-2', status: 'pending' });
    expect(calls).toEqual([
      { kind: 'start' },
      {
        kind: 'continue',
        payload: {
          pendingId: 'pending-1',
          input: { type: 'email', address: 'user@example.com' },
        },
      },
    ]);
  });

  test('reuses an existing pending session', async () => {
    let startCalls = 0;
    const result = await sendEmailOtpCode('user@example.com', 'existing', {
      loginStart: async () => {
        startCalls += 1;
        return { pendingId: 'new' };
      },
      loginContinue: async () => ({ status: 'pending', pendingId: 'existing', method: 'email_otp', message: 'sent' }),
    });
    expect(result).toEqual({ ok: true, pendingId: 'existing', status: 'pending' });
    expect(startCalls).toBe(0);
  });

  test('classifies transport failures from loginContinue', async () => {
    const result = await sendEmailOtpCode('user@example.com', 'existing', {
      loginStart: async () => ({ pendingId: 'p1' }),
      loginContinue: async () => {
        throw { status: 502, message: 'error sending request for url' };
      },
    });
    expect(result).toEqual({ ok: false, kind: 'transport' });
    expect(classifyOtpVerificationError({ status: 502, message: 'gateway timeout' })).toBe('transport');
  });
});
