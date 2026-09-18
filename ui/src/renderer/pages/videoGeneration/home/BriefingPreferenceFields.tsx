import React from 'react';
import { Input, Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import DurationTimelineBar from '../components/DurationTimelineBar';
import {
  BRIEFING_DURATION_MAX_SECS,
  BRIEFING_DURATION_MIN_SECS,
  BRIEFING_DURATION_STEP_SECS,
  BRIEFING_TICKS,
  clampDuration,
} from '../durationBounds';
import BriefingModelFields, { type BriefingPrefsSelectProps } from './BriefingModelFields';
import type { BriefingPreferenceValue, BriefingResearchDepth } from './types';
import styles from './home.module.css';

export interface BriefingPreferenceFieldsProps {
  value: BriefingPreferenceValue;
  disabled?: boolean;
  selectProps?: BriefingPrefsSelectProps;
  onChange: (next: BriefingPreferenceValue) => void;
}

const BriefingPreferenceFields: React.FC<BriefingPreferenceFieldsProps> = ({
  value,
  disabled,
  selectProps,
  onChange,
}) => {
  const { t } = useTranslation();
  const formatSecs = clampDuration(
    value.formatSecs,
    BRIEFING_DURATION_MIN_SECS,
    BRIEFING_DURATION_MAX_SECS,
    BRIEFING_DURATION_STEP_SECS
  );
  const patch = (partial: Partial<BriefingPreferenceValue>) =>
    onChange({ ...value, ...partial });

  return (
    <>
      <div className={styles.preferenceSection}>
        <div className={styles.preferenceLabel}>{t('videoGeneration.briefing.format')}</div>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.formatHint')}</p>
        <div className={styles.durationWrap}>
          <DurationTimelineBar
            value={formatSecs}
            disabled={disabled}
            hideLabel
            min={BRIEFING_DURATION_MIN_SECS}
            max={BRIEFING_DURATION_MAX_SECS}
            step={BRIEFING_DURATION_STEP_SECS}
            ticks={BRIEFING_TICKS}
            hideCredits
            onChange={(next) => patch({ formatSecs: next })}
          />
        </div>
      </div>

      <div className={styles.preferenceSection}>
        <div className={styles.preferenceLabel}>{t('videoGeneration.briefing.depth')}</div>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.depthHint')}</p>
        <Select
          size='small'
          getPopupContainer={() => document.body}
          disabled={disabled}
          value={value.researchDepth}
          onChange={(researchDepth: BriefingResearchDepth) => patch({ researchDepth })}
          {...selectProps}
        >
          <Select.Option value='fast'>{t('videoGeneration.briefing.fast')}</Select.Option>
          <Select.Option value='deep'>{t('videoGeneration.briefing.deep')}</Select.Option>
        </Select>
      </div>

      <div className={styles.preferenceSection}>
        <div className={styles.preferenceLabel}>{t('videoGeneration.briefing.window')}</div>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.windowHint')}</p>
        <Input
          type='number'
          size='small'
          min={1}
          max={168}
          disabled={disabled}
          value={String(value.timeWindowHours)}
          onChange={(raw) =>
            patch({ timeWindowHours: Math.min(168, Math.max(1, Number(raw) || 24)) })
          }
        />
      </div>

      <div className={styles.preferenceSection}>
        <div className={styles.preferenceLabel}>{t('videoGeneration.briefing.sources')}</div>
        <p className={styles.briefingModelHint}>{t('videoGeneration.briefing.sourcesHint')}</p>
        <Input.TextArea
          disabled={disabled}
          autoSize={{ minRows: 2, maxRows: 4 }}
          value={value.sourceUrls}
          onChange={(sourceUrls) => patch({ sourceUrls })}
          placeholder={t('videoGeneration.briefing.sourcesPlaceholder')}
        />
      </div>

      <div className={styles.preferenceSection}>
        <BriefingModelFields
          tts={value.tts}
          image={value.image}
          disabled={disabled}
          selectProps={selectProps}
          onTts={(tts) => patch({ tts })}
          onImage={(image) => patch({ image })}
        />
      </div>
    </>
  );
};

export default BriefingPreferenceFields;
