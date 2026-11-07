import React, { useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Select } from '@arco-design/web-react';
import { Left, Refresh, Send } from '@icon-park/react';
import { PromptComposer } from '../../videoGeneration/home/PromptComposer';
import type { CanvasReferenceDraft } from '../../videoGeneration/home/types';
import type { VideoResolution } from '@renderer/services/videoModelCapabilities';
import styles from './HomeVideoComposer.module.css';

export type HomeVideoModelOption = { value: string; label: string };

export type HomeVideoComposerProps = {
  text: string;
  onTextChange: (value: string) => void;
  references: CanvasReferenceDraft[];
  onAddReferences: (files: File[]) => void;
  onRemoveReference: (localId: string) => void;
  model: string;
  modelOptions: HomeVideoModelOption[];
  onModelChange: (value: string) => void;
  modelsLoading?: boolean;
  /** Distinguishes loading / catalog failure / no plan-eligible model. */
  catalogState?: 'loading' | 'error' | 'no-model' | 'ready';
  /** Catalog rows filtered out because the current plan cannot call them. */
  lockedModelCount?: number;
  /** Manual retry when the media-model catalog failed to load. */
  onRetryModels?: () => void;
  /** Unknown create outcome or confirmed task whose navigation failed. */
  recovery?: {
    kind: 'unknown' | 'navigation-failed';
    checking?: boolean;
    onCheck: () => void;
    onOpen: () => void;
  };
  durationSecs: number;
  durationOptions: number[];
  onDurationChange: (value: number) => void;
  resolution: VideoResolution;
  resolutionOptions: readonly VideoResolution[];
  onResolutionChange: (value: VideoResolution) => void;
  submitting: boolean;
  onSend: () => void;
  onExit: () => void;
};

/**
 * Homepage video composer: the clip-generation capability reuses the real video
 * module prompt editor (image mentions + attachment preview/remove + upload).
 * Model, duration, resolution and reference ids all come from the controls the
 * clip task actually receives — nothing here is decorative.
 */
const HomeVideoComposer: React.FC<HomeVideoComposerProps> = ({
  text,
  onTextChange,
  references,
  onAddReferences,
  onRemoveReference,
  model,
  modelOptions,
  onModelChange,
  modelsLoading = false,
  catalogState = 'ready',
  lockedModelCount = 0,
  onRetryModels,
  recovery,
  durationSecs,
  durationOptions,
  onDurationChange,
  resolution,
  resolutionOptions,
  onResolutionChange,
  submitting,
  onSend,
  onExit,
}) => {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const requestUpload = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFiles = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(event.target.files ?? []).filter((file) => file.type.startsWith('image/'));
      event.target.value = '';
      if (files.length > 0) onAddReferences(files);
    },
    [onAddReferences],
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== 'Enter' || event.shiftKey) return;
      if (event.nativeEvent.isComposing) return;
      event.preventDefault();
      onSend();
    },
    [onSend],
  );

  const modelMissing = !model && !modelsLoading && catalogState === 'ready';

  return (
    <section className={styles.composer} data-testid='guid-home-video-composer'>
      <div className={styles.promptSlot}>
        <PromptComposer
          mode='generate'
          loading={submitting}
          documentName={null}
          setDocumentName={() => undefined}
          canvasReferences={references}
          removeCanvasReference={onRemoveReference}
          cameos={[]}
          removeCameo={() => undefined}
          selectedVerticalSkills={[]}
          removeVerticalSkill={() => undefined}
          activeText={text}
          setActiveText={onTextChange}
          placeholder={t('videoGeneration.create.composer.generatePlaceholder')}
          handlePromptKeyDown={handleKeyDown}
          onRequestUpload={requestUpload}
        />
      </div>
      <div className={styles.controls}>
        <div
          className={styles.params}
          data-testid='guid-home-video-catalog'
          data-catalog-state={catalogState}
          data-locked-count={lockedModelCount}
        >
          <div className={styles.paramGroup}>
            <span className={styles.paramLabel}>
              {t('videoGeneration.create.preferences.modelAndResolution')}
            </span>
            <Select
              className={styles.modelSelect}
              size='mini'
              value={model || undefined}
              onChange={(value) => onModelChange(String(value))}
              loading={modelsLoading}
              disabled={submitting || modelOptions.length === 0}
              placeholder={t('videoGeneration.create.preferences.noModelSelected')}
              options={modelOptions}
              data-testid='guid-home-video-model'
              aria-label={t('videoGeneration.create.preferences.modelAndResolution')}
            />
          </div>
          <div className={styles.paramGroup}>
            <span className={styles.paramLabel}>{t('videoGeneration.create.preferences.clipDuration')}</span>
            {durationOptions.map((secs) => (
              <button
                key={secs}
                type='button'
                className={`${styles.paramChip} ${durationSecs === secs ? styles.paramChipActive : ''}`}
                disabled={submitting}
                onClick={() => onDurationChange(secs)}
              >
                {secs}s
              </button>
            ))}
          </div>
          <div className={styles.paramGroup}>
            <span className={styles.paramLabel}>{t('videoGeneration.create.preferences.resolutionAria')}</span>
            {resolutionOptions.map((value) => (
              <button
                key={value}
                type='button'
                className={`${styles.paramChip} ${resolution === value ? styles.paramChipActive : ''}`}
                disabled={submitting}
                onClick={() => onResolutionChange(value)}
              >
                {value}
              </button>
            ))}
          </div>
          {modelMissing ? (
            <span className={styles.modelHint} data-testid='guid-home-video-model-missing'>
              {t('videoGeneration.create.preferences.noModelSelected')}
            </span>
          ) : null}
          {catalogState === 'loading' && modelsLoading ? (
            <span className={styles.modelHint} data-testid='guid-home-video-model-loading'>
              {t('videoGeneration.create.preferences.loading')}
            </span>
          ) : null}
          {catalogState === 'error' ? (
            <span className={styles.modelHint} data-testid='guid-home-video-model-error'>
              {t('videoGeneration.list.loadError')}
            </span>
          ) : null}
          {catalogState === 'no-model' ? (
            <span className={styles.modelHint} data-testid='guid-home-video-model-locked'>
              {t('videoGeneration.create.modelRequiredAction', {
                defaultValue: 'A video model is required before generating.',
              })}
              {lockedModelCount > 0 ? ` (${lockedModelCount})` : ''}
            </span>
          ) : null}
          {onRetryModels && (catalogState === 'error' || catalogState === 'no-model') ? (
            <button
              type='button'
              className={styles.retryModels}
              data-testid={
                catalogState === 'error'
                  ? 'guid-home-video-model-retry'
                  : 'guid-home-video-model-refresh'
              }
              onClick={onRetryModels}
            >
              <Refresh theme='outline' size={12} />
              <span>{t('common.refresh')}</span>
            </button>
          ) : null}
        </div>
        {recovery ? (
          <div
            className={styles.params}
            data-testid={`guid-home-video-recovery-${recovery.kind}`}
            role='status'
          >
            <span className={styles.modelHint}>{t('common.unknownError')}</span>
            {recovery.kind === 'unknown' ? (
              <button
                type='button'
                className={styles.retryModels}
                data-testid='guid-home-video-recovery-check'
                disabled={recovery.checking}
                onClick={recovery.onCheck}
              >
                <Refresh theme='outline' size={12} />
                <span>{t('videoGeneration.clip.refresh')}</span>
              </button>
            ) : null}
            <button
              type='button'
              className={styles.retryModels}
              data-testid='guid-home-video-recovery-open'
              onClick={recovery.onOpen}
            >
              <span>
                {recovery.kind === 'unknown'
                  ? t('videoGeneration.list.generateRecentTitle')
                  : t('common.retry')}
              </span>
            </button>
          </div>
        ) : null}
        <div className={styles.actions}>
          <button type='button' className={styles.back} onClick={onExit}>
            <Left theme='outline' size={14} />
            <span>{t('common.back', { defaultValue: 'Back' })}</span>
          </button>
          <button
            type='button'
            className={styles.send}
            data-testid='guid-home-video-send'
            disabled={submitting || text.trim().length === 0 || !model || catalogState !== 'ready' || recovery?.kind === 'unknown'}
            onClick={onSend}
            aria-label={t('common.send', { defaultValue: 'Send' })}
          >
            <Send theme='filled' size={16} />
          </button>
        </div>
      </div>
      <input
        ref={fileInputRef}
        type='file'
        accept='image/*'
        multiple
        hidden
        data-testid='guid-home-video-refs-input'
        onChange={handleFiles}
      />
    </section>
  );
};

export default HomeVideoComposer;
