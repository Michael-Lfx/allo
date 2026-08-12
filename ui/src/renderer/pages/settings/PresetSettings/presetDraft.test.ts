import { describe, expect, test } from 'bun:test';
import { parseAgentId, parseKnowledgeBaseId, parsePresetTagId } from '@/common/types/ids';
import { presetDraftSignature, type PresetDraft } from './presetDraft';

const draft = (overrides: Partial<PresetDraft> = {}): PresetDraft => ({
  identity: { name: 'Research', description: '', avatar: '🤖' },
  preferences: { agents: [parseAgentId('018f2a1b-3c4d-7e8f-9012-3456789abcde'), parseAgentId('018f2a1b-3c4d-7e8f-9012-3456789abcdf')], models: [] },
  targets: ['conversation'],
  knowledgeMcp: { policy: { enabled: false, writeback: false, grounded: false }, knowledgeBaseIds: [parseKnowledgeBaseId('018f2a1b-3c4d-7e8f-9012-3456789abcd0'), parseKnowledgeBaseId('018f2a1b-3c4d-7e8f-9012-3456789abcd1')], mcpServerIds: [] },
  tags: { audience: [parsePresetTagId('018f2a1b-3c4d-7e8f-9012-3456789abcd2'), parsePresetTagId('018f2a1b-3c4d-7e8f-9012-3456789abcd3')], scenario: [] },
  instructions: { context: '', routingDescription: '' },
  skills: { selected: ['skill-b', 'skill-a'], pending: [], disabledBuiltin: [] },
  advancedRouting: { fallbackAllowed: false, autoSelectable: false },
  ...overrides,
});

describe('preset draft snapshots', () => {
  test('does not treat set-like ordering as a change', () => {
    const first = draft();
    const second = draft({
      preferences: { agents: [parseAgentId('018f2a1b-3c4d-7e8f-9012-3456789abcdf'), parseAgentId('018f2a1b-3c4d-7e8f-9012-3456789abcde')], models: [] },
      knowledgeMcp: { ...first.knowledgeMcp, knowledgeBaseIds: [parseKnowledgeBaseId('018f2a1b-3c4d-7e8f-9012-3456789abcd1'), parseKnowledgeBaseId('018f2a1b-3c4d-7e8f-9012-3456789abcd0')] },
      tags: { audience: [parsePresetTagId('018f2a1b-3c4d-7e8f-9012-3456789abcd3'), parsePresetTagId('018f2a1b-3c4d-7e8f-9012-3456789abcd2')], scenario: [] },
      skills: { ...first.skills, selected: ['skill-a', 'skill-b'] },
    });

    expect(presetDraftSignature(first)).toBe(presetDraftSignature(second));
  });

  test('keeps target and model ordering semantic', () => {
    const first = draft({ targets: ['conversation', 'execution_step'] });
    const second = draft({ targets: ['execution_step', 'conversation'] });

    expect(presetDraftSignature(first)).not.toBe(presetDraftSignature(second));
  });
});
