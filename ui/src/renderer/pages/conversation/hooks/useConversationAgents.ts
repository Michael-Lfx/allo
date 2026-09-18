

import useSWR from 'swr';
import type { Preset } from '@/common/types/agent/presetTypes';
import {
  fetchPresetCatalog,
  PRESET_CATALOG_SWR_KEY,
} from '@/renderer/hooks/preset/presetCatalog';
import {
  DETECTED_AGENTS_SWR_KEY,
  DETECTED_AGENTS_SWR_OPTIONS,
  fetchDetectedAgents,
} from '@/renderer/utils/model/agentTypes';
import type { AgentMetadata } from '@/renderer/utils/model/agentTypes';

export type UseConversationAgentsResult = {
  /** Detected execution engines (acp, extension, remote, nomi, gemini, etc.) */
  cliAgents: AgentMetadata[];
  /** Reusable configurations from `/api/presets`, kept separate from execution Agents. */
  presets: Preset[];
  /** Loading state */
  isLoading: boolean;
  /** Refresh data */
  refresh: () => Promise<void>;
};

/**
 * Hook to fetch available CLI agents and presets for launch selectors.
 *
 * Two independent data sources:
 *   - Execution engines — from AgentRegistry via IPC (agents.detected)
 *   - Presets — from backend `/api/presets` (merged builtin + user + extension)
 */
export const useConversationAgents = (): UseConversationAgentsResult => {
  // Execution engines from AgentRegistry (shared cache with useDetectedAgents / useGuidAgentSelection)
  const {
    data: cliAgents,
    isLoading: isLoadingAgents,
    mutate,
  } = useSWR<AgentMetadata[]>(DETECTED_AGENTS_SWR_KEY, fetchDetectedAgents, DETECTED_AGENTS_SWR_OPTIONS);

  const {
    data: presetCatalog,
    isLoading: isLoadingPresets,
    mutate: mutatePresets,
  } = useSWR<Preset[]>(PRESET_CATALOG_SWR_KEY, fetchPresetCatalog);
  // Installed market packages remain ordinary user presets after the package
  // market is retired. Only the preset's own enabled state controls launch
  // visibility here.
  const presets = (presetCatalog ?? []).filter((preset) => preset.enabled !== false);

  const refresh = async () => {
    await Promise.all([mutate(), mutatePresets()]);
  };

  return {
    cliAgents: cliAgents || [],
    presets: presets || [],
    isLoading: isLoadingAgents || isLoadingPresets,
    refresh,
  };
};
