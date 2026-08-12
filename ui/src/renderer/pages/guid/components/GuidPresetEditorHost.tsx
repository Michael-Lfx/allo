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
  const { setActivePresetId, activePreset, isExtensionPreset, loadPresets } =
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
        editor={editor}
        editAvatarImage={editAvatarImage}
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
        isExtensionPreset={isExtensionPreset}
        availableBackends={availableBackends}
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
