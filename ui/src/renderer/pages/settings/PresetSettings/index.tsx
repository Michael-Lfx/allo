/**
 * PresetSettings — Settings page for managing presets.
 *
 * Editing permissions by preset type:
 *
 * | Field          | Builtin | Extension | Custom |
 * |----------------|---------|-----------|--------|
 * | Save button    |  no     |  no       |  yes   |
 * | Name           |  no     |  no       |  yes   |
 * | Description    |  no     |  no       |  yes   |
 * | Avatar         |  no     |  no       |  yes   |
 * | Main Agent     |  no     |  no       |  yes   |
 * | Prompt editing |  no     |  no       |  yes   |
 * | Delete         |  no     |  no       |  yes   |
 *
 * Builtin and extension presets are fully read-only. The drawer
 * still renders their skills panel so users can inspect what's bundled,
 * but every editing control (including Save) is disabled.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useSearchParams } from 'react-router-dom';
import { Button } from '@arco-design/web-react';
import { GraduationCap } from 'lucide-react';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import coworkSvg from '@/renderer/assets/icons/cowork.svg';
import { useDetectedAgents, usePresetEditor, usePresetList, usePresetTags } from '@/renderer/hooks/preset';
import CapabilityHubShell from '../capabilityHub/CapabilityHubShell';
import { resolveAvatarImageSrc } from './presetUtils';
import ExpertCatalog from './ExpertCatalog';
import MyExpertCatalog from './MyExpertCatalog';
import PresetEditDrawer from './PresetEditDrawer';
import DeletePresetModal from './DeletePresetModal';
import SkillConfirmModals from './SkillConfirmModals';

type PresetNavigationState = {
  openPresetId?: string;
  openPresetEditor?: boolean;
  createPresetDraft?: {
    name?: string;
    description?: string;
    context?: string;
    skills?: string[];
    avatar?: string;
  };
};
const OPEN_PRESET_EDITOR_INTENT_KEY = 'guid.openPresetEditorIntent';

const PresetSettings: React.FC = () => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const navigationState = (location.state as PresetNavigationState | null) ?? null;
  const highlightId = searchParams.get('highlight');
  const [view, setView] = useState<'catalog' | 'mine'>('catalog');

  const handleHighlightConsumed = useCallback(() => {
    const next = new URLSearchParams(searchParams);
    next.delete('highlight');
    setSearchParams(next, { replace: true });
  }, [searchParams, setSearchParams]);

  const avatarImageMap: Record<string, string> = useMemo(
    () => ({
      'cowork.svg': coworkSvg,
      '\u{1F6E0}\u{FE0F}': coworkSvg,
    }),
    []
  );

  // Compose hooks
  const {
    presets,
    setActivePresetId,
    activePreset,
    isExtensionPreset,
    loadPresets,
    localeKey,
  } = usePresetList();

  const { availableBackends, refreshAgentDetection } = useDetectedAgents();

  const tags = usePresetTags();

  const editor = usePresetEditor({
    localeKey,
    activePreset,
    isExtensionPreset,
    setActivePresetId,
    loadPresets,
    refreshAgentDetection,
    message,
  });

  const editAvatarImage = resolveAvatarImageSrc(editor.editAvatar, avatarImageMap);
  const hasConsumedNavigationIntentRef = useRef(false);

  useEffect(() => {
    if (hasConsumedNavigationIntentRef.current) return;

    if (navigationState?.createPresetDraft) {
      hasConsumedNavigationIntentRef.current = true;
      void editor.handleCreate(navigationState.createPresetDraft);
      return;
    }

    const openPresetFromRoute =
      navigationState?.openPresetEditor && navigationState.openPresetId ? navigationState.openPresetId : null;

    let openPresetFromSession: string | null = null;
    try {
      const rawIntent = sessionStorage.getItem(OPEN_PRESET_EDITOR_INTENT_KEY);
      if (rawIntent) {
        const parsedIntent = JSON.parse(rawIntent) as { presetId?: string; openPresetEditor?: boolean };
        if (parsedIntent.openPresetEditor && parsedIntent.presetId) {
          openPresetFromSession = parsedIntent.presetId;
        }
      }
    } catch (error) {
      console.error('[PresetManagement] Failed to parse preset open intent:', error);
    }

    const targetPresetId = openPresetFromRoute ?? openPresetFromSession;
    if (!targetPresetId) return;
    if (presets.length === 0) return;

    const targetPreset = presets.find((preset) => preset.preset_id === targetPresetId);
    if (!targetPreset) return;

    hasConsumedNavigationIntentRef.current = true;
    try {
      sessionStorage.removeItem(OPEN_PRESET_EDITOR_INTENT_KEY);
    } catch (error) {
      console.error('[PresetManagement] Failed to clear preset open intent:', error);
    }
    void editor.handleEdit(targetPreset);
  }, [presets, editor, navigationState]);

  return (
    <CapabilityHubShell
      hub='presets'
      marketEnabled={false}
      showViewSwitch={false}
      extraActions={
        <Button
          size='small'
          className='flowy-icon-text-btn capability-hub-action-btn capability-hub-action-btn--plain'
          icon={<GraduationCap size={16} strokeWidth={2} />}
          onClick={() => setView((current) => (current === 'mine' ? 'catalog' : 'mine'))}
          data-testid='btn-my-experts'
        >
          {view === 'mine'
            ? t('settings.experts.backToCatalog', { defaultValue: 'Back to experts' })
            : t('settings.capabilityHub.myPresets', { defaultValue: 'My experts' })}
        </Button>
      }
    >
      {messageContext}
      {view === 'mine' ? (
        <MyExpertCatalog
          presets={presets}
          tagById={tags.tagById}
          localeKey={localeKey}
          onOpen={(preset) => void editor.handleEdit(preset)}
          onCreate={() => void editor.handleCreate()}
        />
      ) : (
        <ExpertCatalog
          highlightId={highlightId}
          onHighlightConsumed={handleHighlightConsumed}
          onInstalled={() => void loadPresets()}
        />
      )}

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
          editor.isCreating
            ? false
            : activePreset?.source === 'builtin' || isExtensionPreset(activePreset)
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
        message={message}
      />

    </CapabilityHubShell>
  );
};

export default PresetSettings;
