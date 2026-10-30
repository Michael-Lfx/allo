import { ipcBridge } from '@/common';
import type {
  PresetTag,
  PresetTagDimension,
  CreatePresetTagRequest,
} from '@/common/types/agent/presetTypes';
import { useCallback, useMemo } from 'react';
import useSWR from 'swr';

/** One cache identity for every tag-vocabulary consumer (experts, skills, Guid). */
export const PRESET_TAGS_SWR_KEY = 'presetTags.list';

const EMPTY_TAGS: PresetTag[] = [];

const fetchPresetTags = (): Promise<PresetTag[]> => ipcBridge.presetTags.list.invoke();

/** Loads the merged tag vocabulary and exposes CRUD + per-dimension views. */
export const usePresetTags = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled ?? true;
  const {
    data,
    isLoading: loading,
    mutate,
  } = useSWR<PresetTag[]>(enabled ? PRESET_TAGS_SWR_KEY : null, fetchPresetTags, {
    revalidateOnFocus: false,
    onError: (error) => console.error('Failed to load preset tags:', error),
  });
  const tags = data ?? EMPTY_TAGS;

  const loadTags = useCallback(async () => {
    try {
      await mutate();
    } catch (error) {
      console.error('Failed to load preset tags:', error);
    }
  }, [mutate]);

  const audienceTags = useMemo(
    () => tags.filter((t) => t.dimension === 'audience').sort((a, b) => a.sort_order - b.sort_order),
    [tags]
  );
  const scenarioTags = useMemo(
    () => tags.filter((t) => t.dimension === 'scenario').sort((a, b) => a.sort_order - b.sort_order),
    [tags]
  );

  /** UUIDv7 business ID → PresetTag, for preset wire references. */
  const tagById = useMemo(() => new Map(tags.map((t) => [t.preset_tag_id, t])), [tags]);
  /** Readable catalog key → PresetTag, retained for skill-tag side stores. */
  const tagByKey = useMemo(() => new Map(tags.map((t) => [t.key, t])), [tags]);

  const createTag = useCallback(
    async (req: CreatePresetTagRequest) => {
      const created = await ipcBridge.presetTags.create.invoke(req);
      await loadTags();
      return created;
    },
    [loadTags]
  );

  const renameTag = useCallback(
    async (preset_tag_id: PresetTag['preset_tag_id'], label: string) => {
      await ipcBridge.presetTags.update.invoke({ preset_tag_id, label });
      await loadTags();
    },
    [loadTags]
  );

  const deleteTag = useCallback(
    async (preset_tag_id: PresetTag['preset_tag_id']) => {
      await ipcBridge.presetTags.delete.invoke({ preset_tag_id });
      await loadTags();
    },
    [loadTags]
  );

  return {
    tags,
    audienceTags,
    scenarioTags,
    tagById,
    tagByKey,
    loading,
    loadTags,
    createTag,
    renameTag,
    deleteTag,
  };
};

export type TagDimension = PresetTagDimension;
