/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Collapse,
  InputNumber,
  Modal,
  Select,
  Slider,
  Switch,
  Tooltip,
} from '@arco-design/web-react';
import { Help } from '@icon-park/react';
import type { LocalModelLaunchOptions } from '@common/types/localModel';
import type { ModelStoreModel } from './types';
import { CACHE_TYPE_OPTIONS, defaultLaunchOptions, initialLaunchOptions } from './defaults';

interface LocalModelLaunchModalProps {
  visible: boolean;
  model?: ModelStoreModel;
  busy?: boolean;
  onClose: () => void;
  onStart: (model: ModelStoreModel, options: LocalModelLaunchOptions) => void;
  onStop: (model: ModelStoreModel) => void;
}

interface NumberFieldProps {
  label: string;
  hint?: string;
  value?: number;
  min?: number;
  max?: number;
  step?: number;
  placeholder?: string;
  onChange: (value: number | undefined) => void;
}

/** Question-mark affordance that reveals a parameter hint on hover. */
const HelpTip: React.FC<{ content?: string }> = ({ content }) => {
  if (!content) return null;
  return (
    <Tooltip content={content} position='top' getPopupContainer={() => document.body}>
      <span className='model-store-launch-help' role='img' aria-label={content}>
        <Help theme='outline' size='1em' fill='currentColor' />
      </span>
    </Tooltip>
  );
};

/** Field label paired with an optional hint icon. */
const FieldLabel: React.FC<{ label: string; hint?: string; className?: string }> = ({
  label,
  hint,
  className = 'text-12px text-t-secondary',
}) => (
  <span className={`model-store-launch-label ${className}`}>
    {label}
    <HelpTip content={hint} />
  </span>
);

const NumberField: React.FC<NumberFieldProps> = ({
  label,
  hint,
  value,
  min,
  max,
  step,
  placeholder,
  onChange,
}) => (
  <div className='flex flex-col gap-4px'>
    <FieldLabel label={label} hint={hint} />
    <InputNumber
      value={value}
      min={min}
      max={max}
      step={step}
      placeholder={placeholder}
      onChange={(next) => onChange(typeof next === 'number' ? next : undefined)}
    />
  </div>
);

/**
 * Runtime variants offered by the launcher. The store currently publishes only
 * the `standard` runtime; the chip group renders from this array so adding a
 * variant later needs no layout change.
 */
const RUNTIME_VARIANTS = [
  {
    id: 'standard',
    labelKey: 'modelStore.launch.variantStandard',
    hintKey: 'modelStore.launch.variantStandardHint',
  },
] as const;

/** Launch configuration dialog, mirroring the herdsman chat start form. */
const LocalModelLaunchModal: React.FC<LocalModelLaunchModalProps> = ({
  visible,
  model,
  busy,
  onClose,
  onStart,
  onStop,
}) => {
  const { t } = useTranslation();
  const [options, setOptions] = useState<LocalModelLaunchOptions>({});
  const [variant, setVariant] = useState<string>(RUNTIME_VARIANTS[0].id);

  useEffect(() => {
    if (visible && model) {
      setOptions(initialLaunchOptions(model));
      setVariant(RUNTIME_VARIANTS[0].id);
    }
  }, [model, visible]);

  const running = model?.status === 'running' || model?.status === 'starting';
  const supportsMtp = model?.supportsMtp ?? false;
  const supportsDflash = model?.supportsDflash ?? false;
  const mtpOn = supportsMtp && (options.mtp_speculative_decoding ?? true);
  const dflashOn = supportsDflash && (options.enable_dflash ?? false) && !mtpOn;
  const ngramOn = !mtpOn && !dflashOn && (options.ngram_speculative_decoding ?? false);
  const contextLimit = model?.contextSize && model.contextSize > 0 ? model.contextSize : 262144;

  const set = <K extends keyof LocalModelLaunchOptions>(key: K, value: LocalModelLaunchOptions[K]) =>
    setOptions((current) => ({ ...current, [key]: value }));

  const setDflash = (on: boolean) => {
    setOptions((current) => ({
      ...current,
      enable_dflash: on,
      mtp_speculative_decoding: on ? false : current.mtp_speculative_decoding,
      ngram_speculative_decoding: on ? false : current.ngram_speculative_decoding,
    }));
  };

  const setMtp = (on: boolean) => {
    setOptions((current) => ({
      ...current,
      mtp_speculative_decoding: on,
      enable_dflash: on ? false : current.enable_dflash,
      ngram_speculative_decoding: on ? false : current.ngram_speculative_decoding,
    }));
  };

  const advancedContent = useMemo(
    () => (
      <div className='flex flex-col gap-12px pt-4px'>
        <div className='flex flex-col gap-4px'>
          <FieldLabel label={t('modelStore.launch.variant')} hint={t('modelStore.launch.variantHint')} />
          <div
            className='model-store-launch-variants'
            role='radiogroup'
            aria-label={t('modelStore.launch.variant')}
          >
            {RUNTIME_VARIANTS.map((item) => {
              const active = variant === item.id;
              const disabled = RUNTIME_VARIANTS.length <= 1;
              return (
                <button
                  key={item.id}
                  type='button'
                  role='radio'
                  aria-checked={active}
                  aria-disabled={disabled}
                  className={`model-store-launch-variant-chip${active ? ' is-active' : ''}`}
                  onClick={() => {
                    if (!disabled) setVariant(item.id);
                  }}
                >
                  <span>{t(item.labelKey)}</span>
                  <HelpTip content={t(item.hintKey)} />
                </button>
              );
            })}
          </div>
        </div>

        <div className='grid grid-cols-2 gap-12px'>
          <div className='flex flex-col gap-4px'>
            <FieldLabel label={t('modelStore.launch.cacheTypeK')} hint={t('modelStore.launch.cacheTypeKHint')} />
            <Select
              value={options.cache_type_k}
              onChange={(value) => set('cache_type_k', value)}
              options={CACHE_TYPE_OPTIONS.map((value) => ({ label: value, value }))}
            />
          </div>
          <div className='flex flex-col gap-4px'>
            <FieldLabel label={t('modelStore.launch.cacheTypeV')} hint={t('modelStore.launch.cacheTypeVHint')} />
            <Select
              value={options.cache_type_v}
              onChange={(value) => set('cache_type_v', value)}
              options={CACHE_TYPE_OPTIONS.map((value) => ({ label: value, value }))}
            />
          </div>
          <NumberField
            label={t('modelStore.launch.temperature')}
            hint={t('modelStore.launch.temperatureHint')}
            value={options.temperature}
            min={0}
            max={5}
            step={0.1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('temperature', value)}
          />
          <NumberField
            label={t('modelStore.launch.topP')}
            hint={t('modelStore.launch.topPHint')}
            value={options.top_p}
            min={0}
            max={1}
            step={0.01}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('top_p', value)}
          />
          <NumberField
            label={t('modelStore.launch.topK')}
            hint={t('modelStore.launch.topKHint')}
            value={options.top_k}
            min={0}
            max={1000}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('top_k', value)}
          />
          <NumberField
            label={t('modelStore.launch.repeatPenalty')}
            hint={t('modelStore.launch.repeatPenaltyHint')}
            value={options.repeat_penalty}
            min={0}
            max={5}
            step={0.01}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('repeat_penalty', value)}
          />
          <NumberField
            label={t('modelStore.launch.minP')}
            hint={t('modelStore.launch.minPHint')}
            value={options.min_p}
            min={0}
            max={1}
            step={0.01}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('min_p', value)}
          />
          <NumberField
            label={t('modelStore.launch.parallel')}
            hint={t('modelStore.launch.parallelHint')}
            value={options.parallel}
            min={1}
            max={16}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('parallel', value)}
          />
          <NumberField
            label={t('modelStore.launch.batchSize')}
            hint={t('modelStore.launch.batchSizeHint')}
            value={options.batch_size}
            min={1}
            max={8192}
            step={128}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('batch_size', value)}
          />
          <NumberField
            label={t('modelStore.launch.ubatchSize')}
            hint={t('modelStore.launch.ubatchSizeHint')}
            value={options.ubatch_size}
            min={1}
            max={8192}
            step={128}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('ubatch_size', value)}
          />
          <NumberField
            label={t('modelStore.launch.gpuLayers')}
            hint={t('modelStore.launch.gpuLayersHint')}
            value={options.gpu_layers}
            min={0}
            max={999}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('gpu_layers', value)}
          />
          <NumberField
            label={t('modelStore.launch.threads')}
            hint={t('modelStore.launch.threadsHint')}
            value={options.threads}
            min={1}
            max={256}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('threads', value)}
          />
          <NumberField
            label={t('modelStore.launch.threadsBatch')}
            hint={t('modelStore.launch.threadsBatchHint')}
            value={options.threads_batch}
            min={1}
            max={256}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('threads_batch', value)}
          />
          <NumberField
            label={t('modelStore.launch.cacheRam')}
            hint={t('modelStore.launch.cacheRamHint')}
            value={options.cache_ram}
            min={-1}
            step={1}
            placeholder={t('modelStore.launch.auto')}
            onChange={(value) => set('cache_ram', value)}
          />
        </div>

        <div className='grid grid-cols-2 gap-8px'>
          <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
            <FieldLabel
              label={t('modelStore.launch.noMmap')}
              hint={t('modelStore.launch.noMmapHint')}
              className='text-13px text-t-secondary'
            />
            <Switch checked={options.no_mmap ?? true} onChange={(value) => set('no_mmap', value)} />
          </label>
          <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
            <FieldLabel
              label={t('modelStore.launch.contBatching')}
              hint={t('modelStore.launch.contBatchingHint')}
              className='text-13px text-t-secondary'
            />
            <Switch
              checked={options.cont_batching ?? false}
              onChange={(value) => set('cont_batching', value)}
            />
          </label>
          <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
            <FieldLabel
              label={t('modelStore.launch.noKvOffload')}
              hint={t('modelStore.launch.noKvOffloadHint')}
              className='text-13px text-t-secondary'
            />
            <Switch
              checked={options.no_kv_offload ?? false}
              onChange={(value) => set('no_kv_offload', value)}
            />
          </label>
        </div>

        {supportsDflash && (
          <div className='flex flex-col gap-10px border-t border-t-solid border-[var(--color-border-2)] pt-12px'>
            <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
              <FieldLabel
                label={t('modelStore.launch.dflash')}
                hint={t('modelStore.launch.dflashHint')}
                className='text-13px text-t-secondary'
              />
              <Switch checked={dflashOn} onChange={setDflash} />
            </label>
            {dflashOn && (
              <div className='grid grid-cols-3 gap-12px'>
                <NumberField
                  label={t('modelStore.launch.dflashGpuLayers')}
                  hint={t('modelStore.launch.dflashGpuLayersHint')}
                  value={options.dflash_gpu_layers}
                  min={0}
                  max={999}
                  step={1}
                  onChange={(value) => set('dflash_gpu_layers', value)}
                />
                <NumberField
                  label={t('modelStore.launch.dflashDraftMin')}
                  hint={t('modelStore.launch.dflashDraftMinHint')}
                  value={options.dflash_draft_min}
                  min={0}
                  max={4096}
                  step={1}
                  onChange={(value) => set('dflash_draft_min', value)}
                />
                <NumberField
                  label={t('modelStore.launch.dflashDraftMax')}
                  hint={t('modelStore.launch.dflashDraftMaxHint')}
                  value={options.dflash_draft_max}
                  min={1}
                  max={4096}
                  step={1}
                  onChange={(value) => set('dflash_draft_max', value)}
                />
              </div>
            )}
          </div>
        )}

        {supportsMtp && (
          <div className='flex flex-col gap-10px border-t border-t-solid border-[var(--color-border-2)] pt-12px'>
            <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
              <FieldLabel
                label={t('modelStore.launch.mtp')}
                hint={t('modelStore.launch.mtpHint')}
                className='text-13px text-t-secondary'
              />
              <Switch checked={mtpOn} onChange={setMtp} />
            </label>
            {mtpOn && (
              <NumberField
                label={t('modelStore.launch.mtpDraftMax')}
                hint={t('modelStore.launch.mtpDraftMaxHint')}
                value={options.mtp_draft_max}
                min={1}
                max={16}
                step={1}
                onChange={(value) => set('mtp_draft_max', value)}
              />
            )}
          </div>
        )}

        {!mtpOn && !dflashOn && (
          <div className='flex flex-col gap-10px border-t border-t-solid border-[var(--color-border-2)] pt-12px'>
            <label className='flex items-center justify-between gap-8px text-13px text-t-secondary'>
              <FieldLabel
                label={t('modelStore.launch.ngram')}
                hint={t('modelStore.launch.ngramHint')}
                className='text-13px text-t-secondary'
              />
              <Switch
                checked={options.ngram_speculative_decoding ?? false}
                onChange={(value) => set('ngram_speculative_decoding', value)}
              />
            </label>
            {ngramOn && (
              <div className='grid grid-cols-2 gap-12px'>
                <NumberField
                  label={t('modelStore.launch.specNgramSizeN')}
                  hint={t('modelStore.launch.specNgramSizeNHint')}
                  value={options.spec_ngram_size_n}
                  min={1}
                  max={256}
                  step={1}
                  onChange={(value) => set('spec_ngram_size_n', value)}
                />
                <NumberField
                  label={t('modelStore.launch.draftMin')}
                  hint={t('modelStore.launch.draftMinHint')}
                  value={options.draft_min}
                  min={1}
                  max={4096}
                  step={1}
                  onChange={(value) => set('draft_min', value)}
                />
                <NumberField
                  label={t('modelStore.launch.draftMax')}
                  hint={t('modelStore.launch.draftMaxHint')}
                  value={options.draft_max}
                  min={1}
                  max={4096}
                  step={1}
                  onChange={(value) => set('draft_max', value)}
                />
              </div>
            )}
          </div>
        )}
      </div>
    ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      dflashOn,
      mtpOn,
      ngramOn,
      options,
      supportsDflash,
      supportsMtp,
      t,
      variant,
    ]
  );

  return (
    <Modal
      title={t('modelStore.launch.title')}
      className='model-store-launch-modal'
      visible={visible}
      unmountOnExit
      autoFocus={false}
      alignCenter={false}
      style={{ width: 'max(40vw, 600px)', maxWidth: '94vw' }}
      onCancel={() => {
        if (!busy && !running) onClose();
      }}
      footer={
        <div className='flex w-full items-center justify-end gap-12px'>
          <Button onClick={onClose} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button
            disabled={!model}
            onClick={() => model && setOptions(defaultLaunchOptions(model))}
          >
            {t('modelStore.launch.resetDefaults')}
          </Button>
          {running ? (
            <Button type='primary' status='danger' onClick={() => model && onStop(model)}>
              {t('modelStore.actions.stop')}
            </Button>
          ) : (
            <Button
              type='primary'
              className='model-store-themed-primary'
              loading={busy}
              disabled={!model}
              onClick={() => model && onStart(model, options)}
            >
              {t('modelStore.actions.start')}
            </Button>
          )}
        </div>
      }
    >
      {model && (
        <div className='flex flex-col gap-16px pr-4px'>
          <div className='flex flex-col gap-6px'>
            <FieldLabel
              label={`${t('modelStore.launch.contextSize')} (1 - ${(contextLimit / 1024).toFixed(0)}K)`}
              hint={t('modelStore.launch.contextSizeHint')}
            />
            <div className='flex items-center gap-12px'>
              <Slider
                className='flex-1'
                min={1}
                max={contextLimit}
                step={2048}
                value={options.context_size ?? contextLimit}
                onChange={(value) => set('context_size', value as number)}
              />
              <span className='model-store-launch-context-value shrink-0 rounded-6px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-2)] px-8px py-2px text-12px tabular-nums text-t-primary'>
                {Math.round((options.context_size ?? contextLimit) / 1024)}K
              </span>
            </div>
          </div>

          <Collapse>
            <Collapse.Item name='advanced' header={t('modelStore.launch.advanced')}>
              {advancedContent}
            </Collapse.Item>
          </Collapse>
        </div>
      )}
    </Modal>
  );
};

export default LocalModelLaunchModal;
