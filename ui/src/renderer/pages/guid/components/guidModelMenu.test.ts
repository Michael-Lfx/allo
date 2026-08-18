/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { compositeKey } from '@/common/utils/compositeKey';
import { findChatModelForMenuKey } from './guidModelMenu';

describe('findChatModelForMenuKey', () => {
  const openai = { id: 'openai', name: 'OpenAI' };
  const anthropic = { id: 'anthropic', name: 'Anthropic' };
  const groups = [
    { provider: openai, models: ['gpt-4.1', 'o4-mini'] },
    { provider: anthropic, models: ['claude-sonnet-4'] },
  ];

  test('resolves a composite menu key to the matching provider and model', () => {
    expect(findChatModelForMenuKey(groups, compositeKey('anthropic', 'claude-sonnet-4'))).toEqual({
      provider: anthropic,
      modelName: 'claude-sonnet-4',
    });
  });

  test('ignores disabled placeholder keys', () => {
    expect(findChatModelForMenuKey(groups, 'no-models')).toBeUndefined();
    expect(findChatModelForMenuKey(groups, 'unavailable-default')).toBeUndefined();
  });
});
