/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { normalizeConversationModelErrorCode, providerErrorI18nKey } from './conversationCreateError';

describe('providerErrorI18nKey', () => {
  test('maps PROVIDER_UNAVAILABLE', () => {
    expect(providerErrorI18nKey('PROVIDER_UNAVAILABLE')).toBe('conversation.agentError.codes.PROVIDER_UNAVAILABLE.body');
  });
  test('returns undefined for unrelated codes', () => {
    expect(providerErrorI18nKey('BAD_REQUEST')).toBeUndefined();
  });

  test('normalizes embedded missing-model messages without exposing backend text', () => {
    expect(normalizeConversationModelErrorCode(new Error('Bad request: Nomi conversation has no provider/model configured'))).toBe(
      'MODEL_NOT_CONFIGURED',
    );
  });

  test('normalizes provider availability codes', () => {
    expect(normalizeConversationModelErrorCode(new Error('{"code":"PROVIDER_UNAVAILABLE"}'))).toBe('PROVIDER_UNAVAILABLE');
  });
});
