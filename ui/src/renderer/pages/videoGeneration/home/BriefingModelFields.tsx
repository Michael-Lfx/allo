import React, { useMemo } from 'react';
import { Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/types/ids';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { filterAllowedImageModels } from '../components/modelPreferenceDefaults';
import type { BriefingModelPick } from './types';
import styles from './home.module.css';

export type BriefingPrefsSelectProps = {
  getPopupContainer?: () => HTMLElement;
  triggerProps?: {
    updateOnScroll?: boolean;
    autoFitPosition?: boolean;
    className?: string;
    popupStyle?: React.CSSProperties;
  };
  dropdownMenuStyle?: React.CSSProperties;
  dropdownMenuClassName?: string;
};

export interface BriefingModelFieldsProps {
  tts: BriefingModelPick | null;
  image: BriefingModelPick | null;
  disabled?: boolean;
  selectProps?: BriefingPrefsSelectProps;
  onTts: (value: BriefingModelPick | null) => void;
  onImage: (value: BriefingModelPick | null) => void;
}

function catalogPick(model: string): BriefingModelPick {
  return {
    provider_id: FLOWY_BUILTIN_PROVIDER_ID,
    model,
    voice: null,
  };
}

function optionLabel(id: string, name?: string): string {
  const trimmed = name?.trim();
  return trimmed || formatCloudModelLabel(id);
}

const BriefingModelFields: React.FC<BriefingModelFieldsProps> = ({
  tts,
  image,
  disabled,
  selectProps,
  onTts,
  onImage,
}) => {
  const { t } = useTranslation();
  const { audioModels, imageModels, isLoading } = useMediaModels();
  const loadingLabel = t('videoGeneration.create.preferences.loading', {
    defaultValue: '加载中…',
  });

  const audioOptions = useMemo(() => {
    const options = audioModels.map((model) => ({
      value: model.id,
      label: optionLabel(model.id, model.name),
    }));
    if (tts?.model && !options.some((option) => option.value === tts.model)) {
      options.unshift({ value: tts.model, label: optionLabel(tts.model) });
    }
    return options;
  }, [audioModels, tts?.model]);

  const imageOptions = useMemo(() => {
    const options = filterAllowedImageModels(imageModels).map((model) => ({
      value: model.id,
      label: optionLabel(model.id, model.name),
    }));
    if (image?.model && !options.some((option) => option.value === image.model)) {
      options.unshift({ value: image.model, label: optionLabel(image.model) });
    }
    return options;
  }, [imageModels, image?.model]);

  const ttsValue = audioOptions.some((option) => option.value === tts?.model)
    ? tts?.model
    : undefined;
  const imageValue = imageOptions.some((option) => option.value === image?.model)
    ? image?.model
    : undefined;

  return (
    <div className={styles.briefingModels}>
      <div className={styles.briefingModelBlock}>
        <span className={styles.briefingModelLabel}>{t('videoGeneration.briefing.tts')}</span>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.ttsHint')}</p>
        <Select
          allowClear
          size='small'
          getPopupContainer={() => document.body}
          disabled={disabled || isLoading}
          placeholder={t('videoGeneration.briefing.ttsPlaceholder')}
          value={ttsValue}
          options={audioOptions}
          loading={isLoading}
          notFoundContent={isLoading ? loadingLabel : t('videoGeneration.briefing.ttsEmpty')}
          onChange={(next) => {
            const model = String(next ?? '').trim();
            onTts(model ? catalogPick(model) : null);
          }}
          {...selectProps}
        />
      </div>
      <div className={styles.briefingModelBlock}>
        <span className={styles.briefingModelLabel}>{t('videoGeneration.briefing.image')}</span>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.imageHint')}</p>
        <Select
          allowClear
          size='small'
          getPopupContainer={() => document.body}
          disabled={disabled || isLoading}
          placeholder={t('videoGeneration.briefing.imagePlaceholder')}
          value={imageValue}
          options={imageOptions}
          loading={isLoading}
          notFoundContent={isLoading ? loadingLabel : t('videoGeneration.briefing.imageEmpty')}
          onChange={(next) => {
            const model = String(next ?? '').trim();
            onImage(model ? catalogPick(model) : null);
          }}
          {...selectProps}
        />
      </div>
    </div>
  );
};

export default BriefingModelFields;
