

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, InputNumber, Select, Switch } from '@arco-design/web-react';
import { Delete, Plus } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import type { IProvider } from '@/common/config/storage';
import type { ProviderId } from '@/common/types/ids';
import { useModelProviderList, useProvidersQuery } from '@renderer/hooks/agent/useModelProviderList';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import { formatCloudModelLabel } from '@renderer/utils/model/cloudModelLabel';
import {
  SettingsActionBar,
  SettingsControlGroup,
  SettingsEmptyState,
  SettingsGroup,
  SettingsPageHeader,
  SettingsList,
  SettingsRow,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import SettingsPageWrapper from './components/SettingsPageWrapper';

/** One reference-model row on the wire (snake_case MoaSettings DTO). */
type MoaReferenceDto = {
  provider_id: string;
  model: string;
  max_tokens?: number | null;
  temperature?: number | null;
};

/** Global MoA settings wire shape, persisted as a JSON string under the
 * `moa_settings` client preference (read by the backend agent factory as the
 * default for sessions without an explicit per-session MoA config). */
type MoaSettingsDto = {
  enabled: boolean;
  references: MoaReferenceDto[];
  fanout?: string | null;
  reference_timeout_secs?: number | null;
  reference_max_tokens?: number | null;
  privacy_filter?: '' | 'display' | 'full' | null;
  trace_enabled?: boolean | null;
};

type FanoutMode = 'user_turn' | 'per_iteration' | 'every_n';
type PrivacyFilter = '' | 'display' | 'full';

type MoaReferenceRow = {
  provider_id: string;
  model: string;
  max_tokens?: number;
  temperature?: number;
};

type MoaFormState = {
  enabled: boolean;
  traceEnabled: boolean;
  privacyFilter: PrivacyFilter;
  fanoutMode: FanoutMode;
  everyN: number;
  referenceTimeoutSecs?: number;
  referenceMaxTokens?: number;
  references: MoaReferenceRow[];
};

const DEFAULT_FORM: MoaFormState = {
  enabled: false,
  traceEnabled: false,
  privacyFilter: '',
  fanoutMode: 'user_turn',
  everyN: 2,
  references: [],
};

const EVERY_N_RE = /^every_n:(\d+)$/;

const asOptionalNumber = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isFinite(value) ? value : undefined;

/** Missing / malformed persisted JSON degrades to the defaults form. */
export function parseMoaSettings(raw: string | undefined): MoaFormState {
  if (!raw || !raw.trim()) return { ...DEFAULT_FORM, references: [] };
  let dto: MoaSettingsDto;
  try {
    dto = JSON.parse(raw) as MoaSettingsDto;
  } catch {
    return { ...DEFAULT_FORM, references: [] };
  }
  if (!dto || typeof dto !== 'object') return { ...DEFAULT_FORM, references: [] };

  let fanoutMode: FanoutMode = 'user_turn';
  let everyN = DEFAULT_FORM.everyN;
  if (typeof dto.fanout === 'string') {
    const everyMatch = EVERY_N_RE.exec(dto.fanout);
    if (everyMatch) {
      fanoutMode = 'every_n';
      everyN = Math.max(1, Number(everyMatch[1]));
    } else if (dto.fanout === 'per_iteration') {
      fanoutMode = 'per_iteration';
    }
  }

  const privacyFilter: PrivacyFilter =
    dto.privacy_filter === 'display' || dto.privacy_filter === 'full' ? dto.privacy_filter : '';

  const references: MoaReferenceRow[] = Array.isArray(dto.references)
    ? dto.references
        .filter((ref): ref is MoaReferenceDto => !!ref && typeof ref === 'object')
        .map((ref) => ({
          provider_id: typeof ref.provider_id === 'string' ? ref.provider_id : '',
          model: typeof ref.model === 'string' ? ref.model : '',
          max_tokens: asOptionalNumber(ref.max_tokens),
          temperature: asOptionalNumber(ref.temperature),
        }))
    : [];

  return {
    enabled: dto.enabled === true,
    traceEnabled: dto.trace_enabled === true,
    privacyFilter,
    fanoutMode,
    everyN,
    referenceTimeoutSecs: asOptionalNumber(dto.reference_timeout_secs),
    referenceMaxTokens: asOptionalNumber(dto.reference_max_tokens),
    references,
  };
}

/** Incomplete rows (no provider or model picked yet) are not persisted. */
export function serializeMoaSettings(form: MoaFormState): string {
  const dto: MoaSettingsDto = {
    enabled: form.enabled,
    references: form.references
      .filter((row) => row.provider_id && row.model)
      .map((row) => ({
        provider_id: row.provider_id,
        model: row.model,
        max_tokens: row.max_tokens ?? null,
        temperature: row.temperature ?? null,
      })),
    fanout: form.fanoutMode === 'every_n' ? `every_n:${Math.max(1, Math.round(form.everyN))}` : form.fanoutMode,
    reference_timeout_secs: form.referenceTimeoutSecs ?? null,
    reference_max_tokens: form.referenceMaxTokens ?? null,
    privacy_filter: form.privacyFilter,
    trace_enabled: form.traceEnabled,
  };
  return JSON.stringify(dto);
}

const MoaSettings: React.FC = () => {
  const { t } = useTranslation();
  const [form, setForm] = useState<MoaFormState>(() => ({ ...DEFAULT_FORM, references: [] }));
  const [savedForm, setSavedForm] = useState<MoaFormState>(() => ({ ...DEFAULT_FORM, references: [] }));
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const { getAvailableModels } = useModelProviderList();
  const { data: rawProviders } = useProvidersQuery();
  const providerLabel = useModelSelectorProviderLabel();

  const enabledProviders = useMemo(() => (rawProviders ?? []).filter((p) => p.enabled !== false), [rawProviders]);

  useEffect(() => {
    let cancelled = false;
    void configService.whenReady().then(() => {
      if (cancelled) return;
      const saved = parseMoaSettings(configService.get('moa_settings'));
      setForm(saved);
      setSavedForm(saved);
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const patch = (updates: Partial<MoaFormState>) => setForm((prev) => ({ ...prev, ...updates }));

  const patchReference = (index: number, updates: Partial<MoaReferenceRow>) =>
    setForm((prev) => ({
      ...prev,
      references: prev.references.map((row, i) => (i === index ? { ...row, ...updates } : row)),
    }));

  const addReference = () =>
    setForm((prev) => ({ ...prev, references: [...prev.references, { provider_id: '', model: '' }] }));

  const removeReference = (index: number) =>
    setForm((prev) => ({ ...prev, references: prev.references.filter((_, i) => i !== index) }));

  const save = async () => {
    setSaving(true);
    try {
      await configService.set('moa_settings', serializeMoaSettings(form));
      setSavedForm(form);
      setSaveError(null);
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const findProvider = (providerId: string): IProvider | undefined =>
    enabledProviders.find((p) => p.id === providerId);
  const isDirty = serializeMoaSettings(form) !== serializeMoaSettings(savedForm);

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={t('settings.moa.title')}
          description={t('settings.moa.description')}
        />

        <SettingsGroup title={t('settings.moa.title')} description={t('settings.moa.enabledHint')}>
          <SettingsList>
            <SettingsRow
              label={t('settings.moa.enabled')}
              control={<Switch checked={form.enabled} onChange={(enabled) => patch({ enabled })} />}
              controlLayout='compact'
            />
          </SettingsList>
        </SettingsGroup>

        <SettingsGroup title={t('settings.moa.referencesTitle')} description={t('settings.moa.referencesHint')}>
          {form.references.length === 0 ? (
            <SettingsList>
              <SettingsEmptyState
                title={t('settings.moa.referencesEmpty')}
                action={<Button size='small' className='flowy-icon-text-btn' icon={<Plus theme='outline' size='14' />} onClick={addReference}>{t('settings.moa.addReference')}</Button>}
              />
            </SettingsList>
          ) : (
            <>
              <SettingsList>
                {form.references.map((row, index) => {
                  const provider = findProvider(row.provider_id);
                  const availableModels = provider ? getAvailableModels(provider) : [];
                  const staleModel = Boolean(row.model) && Boolean(provider) && !availableModels.includes(row.model);
                  return (
                    <SettingsRow
                      key={index}
                      label={t('settings.moa.referenceNumber', { index: index + 1 })}
                      controlLayout='compound'
                      control={
                        <div className='flex w-full flex-wrap items-center justify-end gap-8px'>
                          <Select size='small' className='min-w-160px flex-1' placeholder={t('settings.moa.referenceProvider')} value={row.provider_id || undefined} onChange={(provider_id: ProviderId) => patchReference(index, { provider_id, model: '' })}>
                            {enabledProviders.map((p) => <Select.Option key={p.id} value={p.id}>{providerLabel(p)}</Select.Option>)}
                          </Select>
                          <Select size='small' className='min-w-200px flex-[1.2]' placeholder={t('settings.moa.referenceModel')} value={row.model || undefined} disabled={!provider} onChange={(model: string) => patchReference(index, { model })}>
                            {staleModel && <Select.Option key={row.model} value={row.model} disabled>{formatCloudModelLabel(row.model)}</Select.Option>}
                            {availableModels.map((model) => <Select.Option key={model} value={model}>{formatCloudModelLabel(model, provider?.model_descriptions)}</Select.Option>)}
                          </Select>
                          <Button size='small' status='danger' icon={<Delete theme='outline' size='14' />} onClick={() => removeReference(index)} aria-label={t('settings.moa.removeReference')} />
                          <div className='flex w-full flex-wrap justify-end gap-8px text-12px text-t-secondary'>
                            <label className='inline-flex items-center gap-6px'>{t('settings.moa.referenceRowMaxTokens')}<InputNumber size='small' className='w-110px' min={1} step={1} placeholder={t('settings.moa.optional')} value={row.max_tokens} onChange={(value) => patchReference(index, { max_tokens: typeof value === 'number' ? value : undefined })} /></label>
                            <label className='inline-flex items-center gap-6px'>{t('settings.moa.referenceRowTemperature')}<InputNumber size='small' className='w-110px' min={0} max={2} step={0.1} placeholder={t('settings.moa.optional')} value={row.temperature} onChange={(value) => patchReference(index, { temperature: typeof value === 'number' ? value : undefined })} /></label>
                          </div>
                        </div>
                      }
                    />
                  );
                })}
              </SettingsList>
              <SettingsControlGroup className='mt-10px justify-start'>
                <Button size='small' className='flowy-icon-text-btn' icon={<Plus theme='outline' size='14' />} onClick={addReference}>{t('settings.moa.addReference')}</Button>
              </SettingsControlGroup>
            </>
          )}
        </SettingsGroup>

        <SettingsGroup title={t('settings.moa.fanoutSection')}>
          <SettingsList>
            <SettingsRow
              label={t('settings.moa.fanoutSection')}
              description={t('settings.moa.fanoutHint')}
              controlLayout='compound'
              control={<div className='flex w-full flex-wrap items-center justify-end gap-8px'><Select size='small' className='min-w-220px flex-1' value={form.fanoutMode} onChange={(fanoutMode: FanoutMode) => patch({ fanoutMode })}><Select.Option value='user_turn'>{t('settings.moa.fanoutUserTurn')}</Select.Option><Select.Option value='per_iteration'>{t('settings.moa.fanoutPerIteration')}</Select.Option><Select.Option value='every_n'>{t('settings.moa.fanoutEveryN')}</Select.Option></Select>{form.fanoutMode === 'every_n' && <label className='inline-flex items-center gap-6px text-12px text-t-secondary'>{t('settings.moa.fanoutEveryNCount')}<InputNumber size='small' className='w-100px' min={1} step={1} value={form.everyN} onChange={(value) => patch({ everyN: typeof value === 'number' ? Math.max(1, Math.round(value)) : 1 })} /></label>}</div>}
            />
            <SettingsRow label={t('settings.moa.referenceTimeoutSecs')} controlLayout='field' control={<InputNumber className='w-160px' min={1} step={1} placeholder='120' value={form.referenceTimeoutSecs} onChange={(value) => patch({ referenceTimeoutSecs: typeof value === 'number' ? value : undefined })} />} />
            <SettingsRow label={t('settings.moa.referenceMaxTokens')} controlLayout='field' control={<InputNumber className='w-160px' min={1} step={1} placeholder='4096' value={form.referenceMaxTokens} onChange={(value) => patch({ referenceMaxTokens: typeof value === 'number' ? value : undefined })} />} />
          </SettingsList>
        </SettingsGroup>

        <SettingsGroup title={t('settings.moa.privacyFilter')}>
          <SettingsList>
            <SettingsRow label={t('settings.moa.privacyFilter')} description={t('settings.moa.privacyHint')} controlLayout='field' control={<Select className='w-full' value={form.privacyFilter} onChange={(privacyFilter: PrivacyFilter) => patch({ privacyFilter })}><Select.Option value=''>{t('settings.moa.privacyOff')}</Select.Option><Select.Option value='display'>{t('settings.moa.privacyDisplay')}</Select.Option><Select.Option value='full'>{t('settings.moa.privacyFull')}</Select.Option></Select>} />
            <SettingsRow label={t('settings.moa.traceEnabled')} description={t('settings.moa.traceHint')} control={<Switch checked={form.traceEnabled} onChange={(traceEnabled) => patch({ traceEnabled })} />} controlLayout='compact' />
          </SettingsList>
        </SettingsGroup>
        <SettingsActionBar
          visible={isDirty || Boolean(saveError)}
          saveLabel={t('common.save', { defaultValue: 'Save' })}
          onSave={() => void save()}
          resetLabel={t('common.cancel', { defaultValue: 'Cancel' })}
          onReset={() => {
            setForm(savedForm);
            setSaveError(null);
          }}
          loading={saving || loading}
          error={saveError}
        />
      </div>
    </SettingsPageWrapper>
  );
};

export default MoaSettings;
