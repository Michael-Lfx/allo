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
import { Plus } from '@icon-park/react';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import coworkSvg from '@/renderer/assets/icons/cowork.svg';
import { useDetectedAgents, usePresetEditor, usePresetList, usePresetTags } from '@/renderer/hooks/preset';
import CapabilityHubShell, { useCapabilityHubSearch } from '../capabilityHub/CapabilityHubShell';
import { useCapabilityHubRoute } from '../capabilityHub/useCapabilityHubRoute';
import { resolveAvatarImageSrc } from './presetUtils';
import PresetEditDrawer from './PresetEditDrawer';
import PresetListPanel from './PresetListPanel';
import DeletePresetModal from './DeletePresetModal';
import SkillConfirmModals from './SkillConfirmModals';
import TagManagementModal from './TagManagementModal';
import PresetPackageMarketSettings from './PresetPackageMarketSettings';

type PresetNavigationState = {
  openPresetId?: string;
  openPresetEditor?: boolean;
};
const OPEN_PRESET_EDITOR_INTENT_KEY = 'guid.openPresetEditorIntent';

type PresetHubBodyProps = {
  presets: ReturnType<typeof usePresetList>['presets'];
  localeKey: string;
  avatarImageMap: Record<string, string>;
  isExtensionPreset: ReturnType<typeof usePresetList>['isExtensionPreset'];
  editor: ReturnType<typeof usePresetEditor>;
  setActivePresetId: ReturnType<typeof usePresetList>['setActivePresetId'];
  highlightId: string | null;
  onHighlightConsumed: () => void;
  tags: ReturnType<typeof usePresetTags>;
  onManageTags: () => void;
  loadPresets: ReturnType<typeof usePresetList>['loadPresets'];
  addedStateLoading: boolean;
};

const PresetHubBody: React.FC<PresetHubBodyProps> = ({
  presets,
  localeKey,
  avatarImageMap,
  isExtensionPreset,
  editor,
  setActivePresetId,
  highlightId,
  onHighlightConsumed,
  tags,
  onManageTags,
  loadPresets,
  addedStateLoading,
}) => {
  const { view } = useCapabilityHubRoute('presets');
  const { searchQuery, setSearchQuery } = useCapabilityHubSearch();

  if (view === 'installed') {
    return (
      <PresetListPanel
        presets={presets}
        localeKey={localeKey}
        avatarImageMap={avatarImageMap}
        isExtensionPreset={isExtensionPreset}
        onEdit={(preset) => void editor.handleEdit(preset)}
        onDuplicate={(preset) => void editor.handleDuplicate(preset)}
        onCreate={() => void editor.handleCreate()}
        onToggleEnabled={(preset, checked) => void editor.handleToggleEnabled(preset, checked)}
        setActivePresetId={setActivePresetId}
        highlightId={highlightId}
        onHighlightConsumed={onHighlightConsumed}
        audienceTags={tags.audienceTags}
        scenarioTags={tags.scenarioTags}
        tagById={tags.tagById}
        onManageTags={onManageTags}
        hideChrome
        searchQuery={searchQuery}
        onSearchQueryChange={setSearchQuery}
      />
    );
  }

  return (
    <PresetPackageMarketSettings
      onImported={loadPresets}
      presets={presets}
      addedStateLoading={addedStateLoading}
      hideSearch
      searchQuery={searchQuery}
      onSearchQueryChange={setSearchQuery}
    />
  );
};

const PresetSettings: React.FC = () => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const navigationState = (location.state as PresetNavigationState | null) ?? null;
  const highlightId = searchParams.get('highlight');

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
    loadError: presetsLoadError,
    isLoading: presetsLoading,
    localeKey,
  } = usePresetList();

  const { availableBackends, refreshAgentDetection } = useDetectedAgents();

  const tags = usePresetTags();
  const [tagModalVisible, setTagModalVisible] = useState(false);

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
      installedCount={presets.length}
      extraActions={
        <Button
          size='small'
          type='outline'
          className='flowy-icon-text-btn capability-hub-action-btn'
          icon={<Plus size={14} fill='currentColor' />}
          onClick={() => void editor.handleCreate()}
          data-testid='btn-create-preset'
        >
          {t('settings.createPreset', { defaultValue: 'Create Preset' })}
        </Button>
      }
    >
      {messageContext}
      <PresetHubBody
        presets={presets}
        localeKey={localeKey}
        avatarImageMap={avatarImageMap}
        isExtensionPreset={isExtensionPreset}
        editor={editor}
        setActivePresetId={setActivePresetId}
        highlightId={highlightId}
        onHighlightConsumed={handleHighlightConsumed}
        tags={tags}
        onManageTags={() => setTagModalVisible(true)}
        loadPresets={loadPresets}
        addedStateLoading={presetsLoading || Boolean(presetsLoadError)}
      />

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

      <TagManagementModal
        visible={tagModalVisible}
        onClose={() => setTagModalVisible(false)}
        audienceTags={tags.audienceTags}
        scenarioTags={tags.scenarioTags}
        localeKey={localeKey}
        onCreate={tags.createTag}
        onRename={tags.renameTag}
        onDelete={tags.deleteTag}
        message={message}
      />
    </CapabilityHubShell>
  );
};

export default PresetSettings;
