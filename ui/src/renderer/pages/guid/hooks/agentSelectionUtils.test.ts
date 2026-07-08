/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  assistantIdMatches,
  findAssistantById,
  parseCustomAssistantId,
  toPresetAvailableAgent,
} from './agentSelectionUtils';

describe('agentSelectionUtils preset assistant helpers', () => {
  test('parseCustomAssistantId reads custom selection keys', () => {
    expect(parseCustomAssistantId('custom:abc')).toBe('abc');
    expect(parseCustomAssistantId('nomi')).toBeNull();
    expect(parseCustomAssistantId('custom:')).toBeNull();
  });

  test('assistantIdMatches normalizes builtin aliases', () => {
    expect(assistantIdMatches('builtin-cowork', 'cowork')).toBe(true);
    expect(assistantIdMatches('cowork', 'builtin-cowork')).toBe(true);
    expect(assistantIdMatches('other', 'cowork')).toBe(false);
  });

  test('findAssistantById resolves alias ids in the catalog', () => {
    const assistants = [{ id: 'builtin-cowork', name: 'Cowork' }];
    expect(findAssistantById(assistants, 'cowork')?.name).toBe('Cowork');
  });

  test('toPresetAvailableAgent maps preset backend from the catalog row', () => {
    const agent = toPresetAvailableAgent({
      id: 'builtin-cowork',
      name: 'Cowork',
      preset_agent_type: 'nomi',
      avatar: '🤝',
    } as never);
    expect(agent.backend).toBe('nomi');
    expect(agent.custom_agent_id).toBe('builtin-cowork');
    expect(agent.is_preset).toBe(true);
  });
});
