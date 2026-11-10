import React from 'react';
import { Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import {
  DEFAULT_IMAGE_PROMPT_PARAMETERS,
  IMAGE_ASPECT_OPTIONS,
  IMAGE_DETAIL_OPTIONS,
  IMAGE_STYLE_OPTIONS,
  isDefaultImagePromptParameters,
  type ImagePromptParameters,
} from './imagePromptParameters';
import styles from './ImagePromptOptions.module.css';

type ImagePromptOptionsProps = {
  value: ImagePromptParameters;
  onChange: (value: ImagePromptParameters) => void;
};

const ImagePromptOptions: React.FC<ImagePromptOptionsProps> = ({ value, onChange }) => {
  const { t } = useTranslation();
  const optionsOf = (entries: { value: string; labelKey: string }[]) =>
    entries.map((entry) => ({ label: t(entry.labelKey), value: entry.value }));

  return (
    <div className={styles.strip} data-testid='guid-home-image-options'>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t('guid.home.imageParamStyle')}</span>
        <Select
          className={styles.select}
          size='small'
          allowClear
          placeholder={t('guid.home.imageParamChoose')}
          value={value.style ?? undefined}
          options={optionsOf(IMAGE_STYLE_OPTIONS)}
          onChange={(next) => onChange({ ...value, style: (next as string) || null })}
        />
      </label>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t('guid.home.imageParamAspect')}</span>
        <Select
          className={`${styles.select} ${styles.aspectSelect}`}
          size='small'
          allowClear
          placeholder={t('guid.home.imageParamChoose')}
          value={value.aspect ?? undefined}
          options={optionsOf(IMAGE_ASPECT_OPTIONS)}
          onChange={(next) => onChange({ ...value, aspect: (next as string) || null })}
        />
      </label>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t('guid.home.imageParamDetails')}</span>
        <Select
          className={`${styles.select} ${styles.multiSelect}`}
          size='small'
          allowClear
          mode='multiple'
          maxTagCount={1}
          placeholder={t('guid.home.imageParamMultiChoose')}
          value={value.details}
          options={optionsOf(IMAGE_DETAIL_OPTIONS)}
          onChange={(next) => onChange({ ...value, details: (next as string[]) ?? [] })}
        />
      </label>
      {!isDefaultImagePromptParameters(value) ? (
        <button
          type='button'
          className={styles.reset}
          onClick={() => onChange(DEFAULT_IMAGE_PROMPT_PARAMETERS)}
        >
          {t('guid.home.imageParamReset')}
        </button>
      ) : null}
    </div>
  );
};

export default ImagePromptOptions;
