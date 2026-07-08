/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { configService } from '@/common/config/configService';
import type { Assistant } from '@/common/types/agent/assistantTypes';
import type { AgentSource } from '@/renderer/utils/model/agentTypes';
import type { AvailableAgent } from '../types';

/** Save preferred mode to the agent's own config key */
export async function savePreferredMode(agentKey: string, mode: string): Promise<void> {
  try {
    if (agentKey === 'nomi') {
      const config = configService.get('nomi.config');
      await configService.set('nomi.config', { ...config, preferredMode: mode });
    } else if (agentKey !== 'custom') {
      const config = configService.get('acp.config');
      const backendConfig = config?.[agentKey as string] || {};
      await configService.set('acp.config', { ...config, [agentKey]: { ...backendConfig, preferredMode: mode } });
    }
  } catch {
    /* silent */
  }
}

/** Save preferred model ID to the agent's acp.config key */
export async function savePreferredModelId(agentKey: string, model_id: string): Promise<void> {
  try {
    const config = configService.get('acp.config');
    const backendConfig = config?.[agentKey as string] || {};
    await configService.set('acp.config', { ...config, [agentKey]: { ...backendConfig, preferredModelId: model_id } });
  } catch {
    /* silent */
  }
}

/** Save default nomi provider/model so the Guid page restores it next session. */
export async function saveNomiDefaultModel(provider_id: string, use_model: string): Promise<void> {
  try {
    await configService.set('nomi.defaultModel', { id: provider_id, use_model });
  } catch {
    /* silent */
  }
}

/**
 * Get agent key for selection.
 *
 * Rows that are row-scoped (custom ACP / remote agents) use `agent.id` directly
 * as the key — no namespace prefix. Builtin / internal agents keep `backend` or
 * `agent_type` as the key since there is only one row per type.
 *
 * Note: preset *assistants* (not agents) still use a `custom:<assistantId>`
 * form produced inline by `AssistantSelectionArea`. That is a separate
 * selection path that points at the backend-merged assistant catalog, not
 * `AgentRegistry`.
 */
export const getAgentKey = (agent: {
  agent_type: string;
  agent_source?: AgentSource;
  backend?: string;
  id?: string;
  is_preset?: boolean;
}): string => {
  const rowScoped = agent.agent_type === 'remote' || agent.agent_source === 'custom';
  if (rowScoped && agent.id) return agent.id;
  return agent.backend || agent.agent_type;
};

/** Parse `custom:<assistantId>` selection keys from the Guid agent picker. */
export const parseCustomAssistantId = (agentKey: string): string | null => {
  if (!agentKey.startsWith('custom:')) return null;
  const assistantId = agentKey.slice('custom:'.length);
  return assistantId || null;
};

/** Match assistant catalog ids, including builtin- alias normalization. */
export const assistantIdMatches = (recordId: string, targetId: string): boolean => {
  const stripped = targetId.replace(/^builtin-/, '');
  const candidates = new Set([targetId, `builtin-${stripped}`, stripped]);
  return candidates.has(recordId);
};

/** Resolve an assistant row from the catalog using id alias normalization. */
export const findAssistantById = <T extends { id: string }>(assistants: T[], targetId: string): T | undefined =>
  assistants.find((item) => assistantIdMatches(item.id, targetId));

/** Map a catalog assistant row into the Guid preset `AvailableAgent` shape. */
export const toPresetAvailableAgent = (assistant: Assistant): AvailableAgent => ({
  agent_type: assistant.preset_agent_type || 'gemini',
  backend: assistant.preset_agent_type || 'gemini',
  name: assistant.name,
  id: assistant.id,
  custom_agent_id: assistant.id,
  is_preset: true,
  context: '',
  avatar: assistant.avatar,
  presetAgentType: assistant.preset_agent_type,
});
