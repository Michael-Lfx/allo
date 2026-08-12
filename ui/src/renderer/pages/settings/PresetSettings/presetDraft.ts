import type { ModelPreference, PresetKnowledgePolicy, PresetTarget } from '@/common/types/agent/presetTypes';
import type { AgentId, KnowledgeBaseId, McpServerId, PresetTagId } from '@/common/types/ids';
import type { PendingSkill } from './types';

export type PresetDraft = {
  identity: {
    name: string;
    description: string;
    avatar: string;
  };
  preferences: {
    agents: AgentId[];
    models: ModelPreference[];
  };
  targets: PresetTarget[];
  knowledgeMcp: {
    policy: PresetKnowledgePolicy;
    knowledgeBaseIds: KnowledgeBaseId[];
    mcpServerIds: McpServerId[];
  };
  tags: {
    audience: PresetTagId[];
    scenario: PresetTagId[];
  };
  instructions: {
    context: string;
    routingDescription: string;
  };
  skills: {
    selected: string[];
    pending: PendingSkill[];
    disabledBuiltin: string[];
  };
  advancedRouting: {
    fallbackAllowed: boolean;
    autoSelectable: boolean;
  };
};

const sortStrings = (values: readonly string[]) => [...values].sort();

/**
 * Produces a comparison-safe snapshot without changing the order sent to the
 * backend. Set-like fields are sorted; model and target order remains semantic.
 */
export const normalizePresetDraft = (draft: PresetDraft) => ({
  identity: draft.identity,
  preferences: {
    agents: sortStrings(draft.preferences.agents),
    models: draft.preferences.models,
  },
  targets: draft.targets,
  knowledgeMcp: {
    policy: draft.knowledgeMcp.policy,
    knowledgeBaseIds: sortStrings(draft.knowledgeMcp.knowledgeBaseIds),
    mcpServerIds: sortStrings(draft.knowledgeMcp.mcpServerIds),
  },
  tags: {
    audience: sortStrings(draft.tags.audience),
    scenario: sortStrings(draft.tags.scenario),
  },
  instructions: draft.instructions,
  skills: {
    selected: sortStrings(draft.skills.selected),
    pending: [...draft.skills.pending].sort((a, b) => a.path.localeCompare(b.path)),
    disabledBuiltin: sortStrings(draft.skills.disabledBuiltin),
  },
  advancedRouting: draft.advancedRouting,
});

export const presetDraftSignature = (draft: PresetDraft): string => JSON.stringify(normalizePresetDraft(draft));
