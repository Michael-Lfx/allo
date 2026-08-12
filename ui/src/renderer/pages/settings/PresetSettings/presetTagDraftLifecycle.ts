import type { PresetTagPickerHandle } from './PresetTagPicker';

type PendingTagPickerRef = {
  current: Pick<PresetTagPickerHandle, 'resetPendingTag'> | null;
};

/** Coordinates unfinished tag drafts at the preset drawer boundary. */
export const createPresetTagDraftLifecycle = (
  audiencePickerRef: PendingTagPickerRef,
  scenarioPickerRef: PendingTagPickerRef,
  setEditVisible: (visible: boolean) => void,
  handleSave: () => Promise<string | null>
) => {
  const resetPendingTagDrafts = () => {
    audiencePickerRef.current?.resetPendingTag();
    scenarioPickerRef.current?.resetPendingTag();
  };

  return {
    resetPendingTagDrafts,
    closeDrawer: () => {
      resetPendingTagDrafts();
      setEditVisible(false);
    },
    handleDrawerSave: async () => {
      resetPendingTagDrafts();
      return handleSave();
    },
  };
};
