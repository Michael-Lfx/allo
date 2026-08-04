import { ipcBridge } from '@/common';
import type { Message } from '@arco-design/web-react';
import type {
  CreatePresetRequest,
  ModelPreference,
  Preset,
  PresetKnowledgePolicy,
  PresetReference,
  PresetTarget,
  UpdatePresetRequest,
} from '@/common/types/agent/presetTypes';
import type {
  PresetListItem,
  BuiltinAutoSkill,
  PendingSkill,
  PresetSkillCatalogItem,
} from '@/renderer/pages/settings/PresetSettings/types';
import type { ImportedAgentSkill } from '@/renderer/pages/settings/skill/AgentSkillImportDrawer';
import type { AgentSkillImportRow } from '@/renderer/pages/settings/skill/agentSkillImportUtils';
import {
  mergePresetSkillIds,
  resolvePresetSkillIdsForSave,
  uniqueCatalogUserSkillIdForName,
  type SelectedPresetSkill,
} from '@/renderer/pages/settings/PresetSettings/presetSkillBindings';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  parseKnowledgeBaseId,
  parseMcpServerId,
  type AgentId,
  type KnowledgeBaseId,
  type McpServerId,
  type PresetTagId,
} from '@/common/types/ids';

type UsePresetEditorParams = {
  localeKey: string;
  activePreset: PresetListItem | null;
  isExtensionPreset: (preset: PresetListItem | null | undefined) => boolean;
  setActivePresetId: (id: PresetReference | null) => void;
  loadPresets: () => Promise<void>;
  refreshAgentDetection: () => Promise<void>;
  message: Required<ReturnType<typeof Message.useMessage>[0]>;
};

const isBuiltinPreset = (preset: Preset | null | undefined): boolean => preset?.source === 'builtin';

const loadPresetSkillCatalog = async (): Promise<PresetSkillCatalogItem[]> => {
  const catalog = await ipcBridge.fs.listSkillCatalog.invoke();
  return catalog.skills;
};

/**
 * Manages all preset editing state and handlers:
 * create, edit, duplicate, save, delete, and toggle enabled.
 */
export const usePresetEditor = ({
  localeKey,
  activePreset,
  isExtensionPreset,
  setActivePresetId,
  loadPresets,
  refreshAgentDetection,
  message,
}: UsePresetEditorParams) => {
  const { t } = useTranslation();

  // Edit drawer state
  const [editVisible, setEditVisible] = useState(false);
  const [editName, setEditName] = useState('');
  const [editDescription, setEditDescription] = useState('');
  const [editContext, setEditContext] = useState('');
  const [editRoutingDescription, setEditRoutingDescription] = useState('');
  const [editAvatar, setEditAvatar] = useState('');
  // Preset preferences always retain canonical AgentMetadata UUIDv7 IDs.
  const [editAgents, setEditAgents] = useState<AgentId[]>([]);
  const [editModels, setEditModels] = useState<ModelPreference[]>([]);
  const [editTargets, setEditTargets] = useState<PresetTarget[]>(['conversation']);
  const [fallbackAllowed, setFallbackAllowed] = useState(false);
  const [autoSelectable, setAutoSelectable] = useState(false);
  const [knowledgePolicy, setKnowledgePolicy] = useState<PresetKnowledgePolicy>({
    enabled: false,
    mode: 'inherit',
    writeback: false,
    grounded: false,
  });
  const [knowledgeBaseIds, setKnowledgeBaseIds] = useState<KnowledgeBaseId[]>([]);
  const [mcpServerIds, setMcpServerIds] = useState<McpServerId[]>([]);
  const [isCreating, setIsCreating] = useState(false);
  const [deleteConfirmVisible, setDeleteConfirmVisible] = useState(false);
  const [promptViewMode, setPromptViewMode] = useState<'edit' | 'preview'>('preview');

  // Skills-related editing state (shared with editor)
  const [availableSkills, setAvailableSkills] = useState<PresetSkillCatalogItem[]>([]);
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const [pendingSkills, setPendingSkills] = useState<PendingSkill[]>([]);
  const [deletePendingSkillName, setDeletePendingSkillName] = useState<string | null>(null);
  const [deleteCustomSkill, setDeleteCustomSkill] = useState<SelectedPresetSkill | null>(null);

  // Builtin auto-injected skills state
  const [builtinAutoSkills, setBuiltinAutoSkills] = useState<BuiltinAutoSkill[]>([]);
  const [disabledBuiltinSkills, setDisabledBuiltinSkills] = useState<string[]>([]);

  // Tag editing state (audience / scenario UUIDv7 business IDs).
  const [editAudienceTags, setEditAudienceTags] = useState<PresetTagId[]>([]);
  const [editScenarioTags, setEditScenarioTags] = useState<PresetTagId[]>([]);

  const handleEdit = async (preset: PresetListItem) => {
    setIsCreating(false);
    setActivePresetId(preset.preset_id);
    setEditName(preset.name || '');
    setEditDescription(preset.description || '');
    setEditAvatar(preset.avatar || '');
    setEditRoutingDescription(preset.routing_description || '');
    setEditContext(preset.instructions_i18n?.[localeKey] || preset.instructions || '');
    setEditAgents(preset.agent_preferences.map((item) => item.agent_id));
    setEditModels(preset.model_preferences);
    setEditTargets(preset.targets);
    setFallbackAllowed(preset.fallback_allowed);
    setAutoSelectable(preset.auto_selectable);
    setKnowledgePolicy(preset.knowledge_policy);
    setKnowledgeBaseIds(preset.knowledge_bases.map((item) => item.knowledge_base_id));
    setMcpServerIds((preset.mcp_server_ids ?? []).map(parseMcpServerId));
    setEditAudienceTags(preset.audience_tag_ids ?? []);
    setEditScenarioTags(preset.scenario_tag_ids ?? []);
    setPendingSkills([]);
    setDeletePendingSkillName(null);
    setDeleteCustomSkill(null);
    setEditVisible(true);

    try {
      const [skillsList, autoSkills] = await Promise.all([
        loadPresetSkillCatalog(),
        ipcBridge.fs.listBuiltinAutoSkills.invoke(),
      ]);
      setAvailableSkills(skillsList);
      setBuiltinAutoSkills(autoSkills);
      setSelectedSkills(preset.included_skills.map((item) => item.skill_id));
      setDisabledBuiltinSkills(preset.excluded_auto_skills);
    } catch (error) {
      console.error('Failed to load preset skill catalog:', error);
      setAvailableSkills([]);
      setBuiltinAutoSkills([]);
      setSelectedSkills([]);
      setDisabledBuiltinSkills([]);
    }
  };

  // Create preset function
  const handleCreate = async () => {
    setIsCreating(true);
    setActivePresetId(null);
    setEditName('');
    setEditDescription('');
    setEditRoutingDescription('');
    setEditContext('');
    setEditAvatar('\u{1F916}');
    setEditAgents([]);
    setEditModels([]);
    setEditTargets(['conversation']);
    setFallbackAllowed(false);
    setAutoSelectable(false);
    setKnowledgePolicy({ enabled: false, mode: 'inherit', writeback: false, grounded: false });
    setKnowledgeBaseIds([]);
    setMcpServerIds([]);
    setSelectedSkills([]);
    setPendingSkills([]);
    setDeletePendingSkillName(null);
    setDeleteCustomSkill(null);
    setDisabledBuiltinSkills([]);
    setEditAudienceTags([]);
    setEditScenarioTags([]);
    setPromptViewMode('edit');
    setEditVisible(true);

    // Load available skills list and builtin auto skills
    try {
      const [skillsList, autoSkills] = await Promise.all([
        loadPresetSkillCatalog(),
        ipcBridge.fs.listBuiltinAutoSkills.invoke(),
      ]);
      setAvailableSkills(skillsList);
      setBuiltinAutoSkills(autoSkills);
    } catch (error) {
      console.error('Failed to load skills:', error);
      setAvailableSkills([]);
      setBuiltinAutoSkills([]);
    }
  };

  // Duplicate preset function
  const handleDuplicate = async (preset: PresetListItem) => {
    setIsCreating(true);
    setActivePresetId(null);
    setEditName(`${preset.name_i18n?.[localeKey] || preset.name} (Copy)`);
    setEditDescription(preset.description_i18n?.[localeKey] || preset.description || '');
    setEditAvatar(preset.avatar || '\u{1F916}');
    setEditRoutingDescription(preset.routing_description || '');
    setEditContext(preset.instructions_i18n?.[localeKey] || preset.instructions || '');
    setEditAgents(preset.agent_preferences.map((item) => item.agent_id));
    setEditModels(preset.model_preferences);
    setEditTargets(preset.targets);
    setFallbackAllowed(preset.fallback_allowed);
    setAutoSelectable(preset.auto_selectable);
    setKnowledgePolicy(preset.knowledge_policy);
    setKnowledgeBaseIds(preset.knowledge_bases.map((item) => item.knowledge_base_id));
    setMcpServerIds((preset.mcp_server_ids ?? []).map(parseMcpServerId));
    setEditAudienceTags(preset.audience_tag_ids ?? []);
    setEditScenarioTags(preset.scenario_tag_ids ?? []);
    setPromptViewMode('edit');
    setEditVisible(true);

    try {
      const [skillsList, autoSkills] = await Promise.all([
        loadPresetSkillCatalog(),
        ipcBridge.fs.listBuiltinAutoSkills.invoke(),
      ]);
      setAvailableSkills(skillsList);
      setBuiltinAutoSkills(autoSkills);
      setSelectedSkills(preset.included_skills.map((item) => item.skill_id));
      setDisabledBuiltinSkills(preset.excluded_auto_skills);
    } catch (error) {
      console.error('Failed to load preset content for duplication:', error);
      setEditContext('');
      setAvailableSkills([]);
      setBuiltinAutoSkills([]);
      setSelectedSkills([]);
      setDisabledBuiltinSkills([]);
    }
  };

  const handleSave = async () => {
    try {
      // Validate required fields
      if (!editName.trim()) {
        message.error(t('settings.presetNameRequired', { defaultValue: 'Preset name is required' }));
        return;
      }
      if (editTargets.length === 0) {
        message.error(t('settings.presetTargetRequired', { defaultValue: 'Select at least one application target' }));
        return;
      }

      let catalogSkills = availableSkills;
      let pendingSkillsForSave = pendingSkills;

      // Import a pending Skill unless it already carries its canonical ID or
      // a unique user-owned catalog entry can prove the same identity. A
      // same-name builtin or ambiguous user entry must not decide for us.
      if (pendingSkills.length > 0) {
        const skillsToImport = pendingSkills.filter(
          (pending) => !pending.skillId && !uniqueCatalogUserSkillIdForName(availableSkills, pending.name),
        );

        if (skillsToImport.length > 0) {
          for (const pendingSkill of skillsToImport) {
            try {
              const result = await ipcBridge.fs.importSkillWithSymlink.invoke({ skill_path: pendingSkill.path });
              const names = result.skill_names?.length
                ? result.skill_names
                : result.skill_name
                  ? [result.skill_name]
                  : [pendingSkill.name];
              const skillId = result.skill_ids?.[names.indexOf(pendingSkill.name)];
              if (skillId) {
                pendingSkillsForSave = pendingSkillsForSave.map((pending) =>
                  pending.path === pendingSkill.path && pending.name === pendingSkill.name
                    ? { ...pending, skillId }
                    : pending,
                );
              }
            } catch (error) {
              console.error(`Failed to import skill "${pendingSkill.name}":`, error);
              message.error(t('settings.presetSkillImportFailed', { name: pendingSkill.name }));
              return;
            }
          }
          catalogSkills = await loadPresetSkillCatalog();
          setAvailableSkills(catalogSkills);
        }
      }

      const selection = resolvePresetSkillIdsForSave(selectedSkills, pendingSkillsForSave, catalogSkills);
      if (selection.unresolvedPendingSkillNames.length > 0) {
        message.error(
          t('settings.presetImportedSkillUnavailable', {
            names: selection.unresolvedPendingSkillNames.join(', '),
          }),
        );
        return;
      }

      const content: CreatePresetRequest = {
        name: editName,
        description: editDescription || undefined,
        routing_description: editRoutingDescription || undefined,
        instructions: editContext,
        instructions_i18n: { [localeKey]: editContext },
        avatar: editAvatar || undefined,
        fallback_allowed: fallbackAllowed,
        targets: editTargets,
        agent_preferences: editAgents.map((agent_id) => ({ agent_id, required: false })),
        model_preferences: editModels,
        included_skills: selection.skillIds.map((skill_id) => ({ skill_id, required: false })),
        excluded_auto_skills: disabledBuiltinSkills,
        knowledge_policy: knowledgePolicy,
        knowledge_bases: knowledgeBaseIds.map((knowledge_base_id) => ({
          knowledge_base_id: parseKnowledgeBaseId(knowledge_base_id),
          required: false,
        })),
        mcp_server_ids: mcpServerIds.map(String),
        audience_tag_ids: editAudienceTags,
        scenario_tag_ids: editScenarioTags,
      };

      if (isCreating) {
        // Create new preset via backend
        const created = await ipcBridge.presets.create.invoke(content);
        if (autoSelectable) {
          await ipcBridge.presets.setState.invoke({
            preset_id: created.preset_id,
            auto_selectable: true,
          });
        }

        setActivePresetId(created.preset_id);
        await loadPresets();
        message.success(t('common.createSuccess', { defaultValue: 'Created successfully' }));
      } else {
        // Update existing preset via backend
        if (!activePreset) return;

        const updateRequest: UpdatePresetRequest = content;
        await ipcBridge.presets.update.invoke({
          preset_id: activePreset.preset_id,
          ...updateRequest,
        });
        await ipcBridge.presets.setState.invoke({
          preset_id: activePreset.preset_id,
          auto_selectable: autoSelectable,
        });

        await loadPresets();
        message.success(t('common.saveSuccess', { defaultValue: 'Saved successfully' }));
      }

      setEditVisible(false);
      setPendingSkills([]);
      await refreshAgentDetection();
    } catch (error) {
      console.error('Failed to save preset:', error);
      message.error(t('common.failed', { defaultValue: 'Failed' }));
    }
  };

  const handleDeleteClick = () => {
    if (!activePreset) return;
    // Cannot delete builtin presets
    if (isBuiltinPreset(activePreset)) {
      message.warning(t('settings.cannotDeleteBuiltin', { defaultValue: 'Cannot delete builtin presets' }));
      return;
    }
    // Extension presets are read-only
    if (isExtensionPreset(activePreset)) {
      message.warning(
        t('settings.extensionPresetReadonly', {
          defaultValue: 'Extension presets are read-only. You can duplicate it and edit the copy.',
        })
      );
      return;
    }
    setDeleteConfirmVisible(true);
  };

  const handleDeleteConfirm = async () => {
    if (!activePreset) return;
    try {
      // Delete the backend-owned preset record. Conversation snapshots remain
      // immutable and continue to describe historical launches.
      await ipcBridge.presets.delete.invoke({ preset_id: activePreset.preset_id });

      // Reload preset list
      await loadPresets();
      setDeleteConfirmVisible(false);
      setEditVisible(false);
      message.success(t('common.success', { defaultValue: 'Success' }));
      await refreshAgentDetection();
    } catch (error) {
      console.error('Failed to delete preset:', error);
      message.error(t('common.failed', { defaultValue: 'Failed' }));
    }
  };

  // Toggle preset enabled state via override (works for all sources except extension)
  const handleToggleEnabled = async (preset: PresetListItem, enabled: boolean) => {
    if (isExtensionPreset(preset)) {
      message.warning(
        t('settings.extensionPresetReadonly', {
          defaultValue: 'Extension presets are read-only. You can duplicate it and edit the copy.',
        })
      );
      return;
    }

    try {
      await ipcBridge.presets.setState.invoke({ preset_id: preset.preset_id, enabled });
      await loadPresets();
      await refreshAgentDetection();
    } catch (error) {
      console.error('Failed to toggle preset:', error);
      message.error(t('common.failed', { defaultValue: 'Failed' }));
    }
  };

  const handleImportAgentSkills = useCallback(async (rows: AgentSkillImportRow[]): Promise<ImportedAgentSkill[]> => {
    const imported: ImportedAgentSkill[] = [];

    for (const row of rows) {
      if (row.alreadyImported) {
        imported.push({
          name: row.name,
          description: row.description,
          path: row.path,
          source: row.source,
          sourceName: row.sourceName,
          alreadyImported: true,
        });
        continue;
      }

      const result = await ipcBridge.fs.importSkillWithSymlink.invoke({ skill_path: row.path });
      const names = result.skill_names?.length ? result.skill_names : result.skill_name ? [result.skill_name] : [row.name];
      for (const [index, name] of names.entries()) {
        imported.push({
          name,
          description: row.description,
          path: row.path,
          source: row.source,
          sourceName: row.sourceName,
          alreadyImported: false,
          skillId: result.skill_ids?.[index],
        });
      }
    }

    if (imported.length > 0) {
      const skillsList = await loadPresetSkillCatalog();
      setAvailableSkills(skillsList);
      const importedSkillIds = imported.flatMap((skill) => {
        if (skill.skillId) return [skill.skillId];
        const skillId = uniqueCatalogUserSkillIdForName(skillsList, skill.name);
        return skillId ? [skillId] : [];
      });
      setSelectedSkills((current) =>
        mergePresetSkillIds(current, importedSkillIds),
      );
    }

    return imported;
  }, []);

  return {
    // Edit drawer state
    editVisible,
    setEditVisible,
    editName,
    setEditName,
    editDescription,
    setEditDescription,
    editContext,
    setEditContext,
    editAvatar,
    setEditAvatar,
    editRoutingDescription,
    setEditRoutingDescription,
    editAgents,
    setEditAgents,
    editModels,
    setEditModels,
    editTargets,
    setEditTargets,
    fallbackAllowed,
    setFallbackAllowed,
    autoSelectable,
    setAutoSelectable,
    knowledgePolicy,
    setKnowledgePolicy,
    knowledgeBaseIds,
    setKnowledgeBaseIds,
    mcpServerIds,
    setMcpServerIds,
    isCreating,
    deleteConfirmVisible,
    setDeleteConfirmVisible,
    promptViewMode,
    setPromptViewMode,

    // Skills editing state
    availableSkills,
    setAvailableSkills,
    selectedSkills,
    setSelectedSkills,
    pendingSkills,
    setPendingSkills,
    deletePendingSkillName,
    setDeletePendingSkillName,
    deleteCustomSkill,
    setDeleteCustomSkill,

    // Builtin auto-injected skills state
    builtinAutoSkills,
    disabledBuiltinSkills,
    setDisabledBuiltinSkills,

    // Tag editing state
    editAudienceTags,
    setEditAudienceTags,
    editScenarioTags,
    setEditScenarioTags,

    // Handlers
    handleEdit,
    handleCreate,
    handleDuplicate,
    handleSave,
    handleImportAgentSkills,
    handleDeleteClick,
    handleDeleteConfirm,
    handleToggleEnabled,
  };
};
