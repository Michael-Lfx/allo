/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 *
 * GuidPresetEditorHost — hosts the editor modal tree (PresetEditDrawer +
 * DeletePresetModal + SkillConfirmModals.
 *
 * Extracted from PresetSelectionArea so the entry page can render these
 * independently of the retired preset card grid.
 */

import coworkSvg from '@/renderer/assets/icons/cowork.svg';
import { useDetectedAgents, usePresetEditor, usePresetList, usePresetTags } from '@/renderer/hooks/preset';
import PresetEditDrawer from '@/renderer/pages/settings/PresetSettings/PresetEditDrawer';
import DeletePresetModal from '@/renderer/pages/settings/PresetSettings/DeletePresetModal';
import SkillConfirmModals from '@/renderer/pages/settings/PresetSettings/SkillConfirmModals';
import { resolveAvatarImageSrc } from '@/renderer/pages/settings/PresetSettings/presetUtils';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import type { EffectiveAgentInfo } from '../types';
import React from 'react';
import { useTranslation } from 'react-i18next';

export interface GuidPresetEditorHostProps {
  localeKey: string;
  currentEffectiveAgentInfo: EffectiveAgentInfo;
}

const avatarImageMap: Record<string, string> = {
  'cowork.svg': coworkSvg,
  '\u{1F6E0}\u{FE0F}': coworkSvg,
};

const GuidPresetEditorHost: React.FC<GuidPresetEditorHostProps> = ({
  localeKey,
  currentEffectiveAgentInfo,
}) => {
  const { t } = useTranslation();
  const [agentMessage, agentMessageContext] = useArcoMessage({ maxCount: 10 });

  // Internal usePresetList owns the drawer editor's working state.
  const { activePresetId, setActivePresetId, activePreset, isExtensionPreset, loadPresets } =
    usePresetList();
  const { availableBackends, refreshAgentDetection } = useDetectedAgents();
  const tags = usePresetTags();

  const editor = usePresetEditor({
    localeKey,
    activePreset,
    isExtensionPreset,
    setActivePresetId,
    loadPresets,
    refreshAgentDetection,
    message: agentMessage,
  });

  const editAvatarImage = resolveAvatarImageSrc(editor.editAvatar, avatarImageMap);

  // ── Fallback notice ──
  const fallbackNotice = currentEffectiveAgentInfo.isFallback ? (
    <div
      className='mb-12px px-12px py-8px rd-8px text-12px flex items-center gap-8px'
      style={{
        background: 'rgb(var(--warning-1))',
        border: '1px solid rgb(var(--warning-3))',
        color: 'rgb(var(--warning-6))',
      }}
    >
      <span>
        {t('guid.agentFallbackNotice', {
          original:
            currentEffectiveAgentInfo.originalType.charAt(0).toUpperCase() +
            currentEffectiveAgentInfo.originalType.slice(1),
          fallback:
            currentEffectiveAgentInfo.agent_type.charAt(0).toUpperCase() +
            currentEffectiveAgentInfo.agent_type.slice(1),
          defaultValue: `${currentEffectiveAgentInfo.originalType.charAt(0).toUpperCase() + currentEffectiveAgentInfo.originalType.slice(1)} is unavailable, using ${currentEffectiveAgentInfo.agent_type.charAt(0).toUpperCase() + currentEffectiveAgentInfo.agent_type.slice(1)} instead.`,
        })}
      </span>
    </div>
  ) : null;

  // ── Modal tree ──
  const modalTree = (
    <>
      {agentMessageContext}
      <PresetEditDrawer
        editVisible={editor.editVisible}
        setEditVisible={editor.setEditVisible}
        isCreating={editor.isCreating}
        editName={editor.editName}
        setEditName={editor.setEditName}
        editDescription={editor.editDescription}
        setEditDescription={editor.setEditDescription}
        editRoutingDescription={editor.editRoutingDescription}
        setEditRoutingDescription={editor.setEditRoutingDescription}
        editAvatar={editor.editAvatar}
        setEditAvatar={editor.setEditAvatar}
        editAvatarImage={editAvatarImage}
        editAgents={editor.editAgents}
        setEditAgents={editor.setEditAgents}
        editModels={editor.editModels}
        setEditModels={editor.setEditModels}
        editTargets={editor.editTargets}
        setEditTargets={editor.setEditTargets}
        fallbackAllowed={editor.fallbackAllowed}
        setFallbackAllowed={editor.setFallbackAllowed}
        autoSelectable={editor.autoSelectable}
        setAutoSelectable={editor.setAutoSelectable}
        knowledgePolicy={editor.knowledgePolicy}
        setKnowledgePolicy={editor.setKnowledgePolicy}
        knowledgeBaseIds={editor.knowledgeBaseIds}
        setKnowledgeBaseIds={editor.setKnowledgeBaseIds}
        mcpServerIds={editor.mcpServerIds}
        setMcpServerIds={editor.setMcpServerIds}
        editContext={editor.editContext}
        setEditContext={editor.setEditContext}
        promptViewMode={editor.promptViewMode}
        setPromptViewMode={editor.setPromptViewMode}
        availableSkills={editor.availableSkills}
        selectedSkills={editor.selectedSkills}
        setSelectedSkills={editor.setSelectedSkills}
        pendingSkills={editor.pendingSkills}
        setDeletePendingSkillName={editor.setDeletePendingSkillName}
        setDeleteCustomSkill={editor.setDeleteCustomSkill}
        builtinAutoSkills={editor.builtinAutoSkills}
        disabledBuiltinSkills={editor.disabledBuiltinSkills}
        setDisabledBuiltinSkills={editor.setDisabledBuiltinSkills}
        editAudienceTags={editor.editAudienceTags}
        setEditAudienceTags={editor.setEditAudienceTags}
        editScenarioTags={editor.editScenarioTags}
        setEditScenarioTags={editor.setEditScenarioTags}
        audienceTags={tags.audienceTags}
        scenarioTags={tags.scenarioTags}
        onCreateTag={tags.createTag}
        readOnly={
          editor.isCreating ? false : activePreset?.source === 'builtin' || isExtensionPreset(activePreset)
        }
        localeKey={localeKey}
        activePreset={activePreset}
        activePresetId={activePresetId}
        isExtensionPreset={isExtensionPreset}
        availableBackends={availableBackends}
        handleSave={editor.handleSave}
        onImportAgentSkills={editor.handleImportAgentSkills}
        handleDeleteClick={editor.handleDeleteClick}
        handleDuplicate={(preset) => void editor.handleDuplicate(preset)}
      />
      <DeletePresetModal
        visible={editor.deleteConfirmVisible}
        onCancel={() => editor.setDeleteConfirmVisible(false)}
        onConfirm={editor.handleDeleteConfirm}
        activePreset={activePreset}
        avatarImageMap={avatarImageMap}
      />
      <SkillConfirmModals
        deletePendingSkillName={editor.deletePendingSkillName}
        setDeletePendingSkillName={editor.setDeletePendingSkillName}
        pendingSkills={editor.pendingSkills}
        setPendingSkills={editor.setPendingSkills}
        deleteCustomSkill={editor.deleteCustomSkill}
        setDeleteCustomSkill={editor.setDeleteCustomSkill}
        selectedSkills={editor.selectedSkills}
        setSelectedSkills={editor.setSelectedSkills}
        message={agentMessage}
      />
    </>
  );

  return (
    <>
      {fallbackNotice}
      {modalTree}
    </>
  );
};

export default GuidPresetEditorHost;
