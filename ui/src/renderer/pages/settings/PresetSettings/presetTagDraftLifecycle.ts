import type { PresetTagPickerHandle } from './PresetTagPicker';

type PendingTagPickerRef = {
  current: Pick<PresetTagPickerHandle, 'resetPendingTag'> | null;
};

/** Coordinates unfinished tag drafts at the preset drawer boundary. */
export const createPresetTagDraftLifecycle = (
  tagPickerRef: PendingTagPickerRef,
  setEditVisible: (visible: boolean) => void,
  handleSave: () => Promise<string | null>
) => {
  const resetPendingTagDrafts = () => {
    tagPickerRef.current?.resetPendingTag();
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
