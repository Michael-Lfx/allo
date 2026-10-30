import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { Plus } from '@icon-park/react';
import type { Preset, PresetTag } from '@/common/types/agent/presetTypes';
import { useCapabilityHubSearch } from '../capabilityHub/CapabilityHubShell';
import ExpertTile, { EXPERT_CARD_GRID_COLS } from './ExpertTile';
import { isEmoji, resolveAvatarImageSrc } from './presetUtils';

type MyExpertCatalogProps = {
  presets: Preset[];
  tagById: Map<string, PresetTag>;
  localeKey: string;
  avatarImageMap: Record<string, string>;
  onOpen: (preset: Preset) => void;
  onCreate: () => void;
};

const MyExpertCatalog: React.FC<MyExpertCatalogProps> = ({
  presets,
  tagById,
  localeKey,
  avatarImageMap,
  onOpen,
  onCreate,
}) => {
  const { t } = useTranslation();
  const { searchQuery } = useCapabilityHubSearch();

  const mine = useMemo(() => presets.filter((preset) => preset.source === 'user'), [presets]);

  const visible = useMemo(() => {
    const needle = searchQuery.trim().toLowerCase();
    if (!needle) return mine;
    return mine.filter((preset) =>
      [preset.name_i18n?.[localeKey] || preset.name, preset.description_i18n?.[localeKey] || preset.description || '']
        .join(' ')
        .toLowerCase()
        .includes(needle)
    );
  }, [mine, searchQuery, localeKey]);

  const tagLabels = (preset: Preset) =>
    [...(preset.audience_tag_ids ?? []), ...(preset.scenario_tag_ids ?? [])]
      .map((presetTagId) => tagById.get(presetTagId))
      .filter((tag): tag is PresetTag => Boolean(tag))
      .map((tag) => tag.label_i18n?.[localeKey] || tag.label);

  const emptyClassName =
    'flex min-h-220px items-center justify-center rd-8px border border-dashed border-arco-2 bg-2 px-20px py-32px text-center text-13px text-t-secondary';

  return (
    <div className='flex flex-col gap-16px pb-16px' data-testid='my-expert-catalog'>
      <div className='flex justify-end'>
        <Button
          type='secondary'
          size='small'
          className='flowy-icon-text-btn'
          icon={<Plus size={14} fill='currentColor' />}
          onClick={onCreate}
          data-testid='btn-create-expert'
        >
          {t('settings.experts.create', { defaultValue: 'Create expert' })}
        </Button>
      </div>
      {mine.length === 0 ? (
        <div className={emptyClassName}>
          {t('settings.experts.mineEmpty', { defaultValue: "You haven't created any experts yet." })}
        </div>
      ) : visible.length === 0 ? (
        <div className={emptyClassName}>
          {t('settings.experts.empty', { defaultValue: 'No experts match the current search.' })}
        </div>
      ) : (
        <div className='grid gap-16px' style={{ gridTemplateColumns: EXPERT_CARD_GRID_COLS }}>
          {visible.map((preset) => (
            <ExpertTile
              key={preset.preset_id}
              testId={`my-expert-card-${preset.preset_id}`}
              name={preset.name_i18n?.[localeKey] || preset.name}
              summary={preset.description_i18n?.[localeKey] || preset.description || ''}
              tags={tagLabels(preset)}
              avatarImageSrc={resolveAvatarImageSrc(preset.avatar, avatarImageMap)}
              avatarEmoji={preset.avatar && isEmoji(preset.avatar.trim()) ? preset.avatar.trim() : undefined}
              onOpen={() => onOpen(preset)}
            />
          ))}
        </div>
      )}
    </div>
  );
};

export default MyExpertCatalog;
