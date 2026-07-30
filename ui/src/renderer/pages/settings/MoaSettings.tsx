

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, InputNumber, Message, Select, Switch, Typography } from '@arco-design/web-react';
import { Delete, Plus } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import type { IProvider } from '@/common/config/storage';
import type { ProviderId } from '@/common/types/ids';
import { useModelProviderList, useProvidersQuery } from '@renderer/hooks/agent/useModelProviderList';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
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
      setForm(parseMoaSettings(configService.get('moa_settings')));
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
      Message.success(t('settings.moa.saved'));
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const findProvider = (providerId: string): IProvider | undefined =>
    enabledProviders.find((p) => p.id === providerId);

  return (
    <SettingsPageWrapper>
      <div className='flex flex-col gap-20px max-w-720px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('settings.moa.title')}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('settings.moa.description')}
          </Typography.Paragraph>
        </div>

        <div className='flex flex-col gap-14px'>
          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px font-500'>{t('settings.moa.enabled')}</span>
            <Switch checked={form.enabled} onChange={(enabled) => patch({ enabled })} />
          </div>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('settings.moa.enabledHint')}
          </Typography.Paragraph>

          {/* Reference model slots */}
          <Typography.Text className='text-t-primary text-14px font-500 mt-4px'>
            {t('settings.moa.referencesTitle')}
          </Typography.Text>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('settings.moa.referencesHint')}
          </Typography.Paragraph>

          {form.references.length === 0 && (
            <div className='rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 px-12px py-10px text-12px text-t-tertiary'>
              {t('settings.moa.referencesEmpty')}
            </div>
          )}

          {form.references.map((row, index) => {
            const provider = findProvider(row.provider_id);
            const availableModels = provider ? getAvailableModels(provider) : [];
            const staleModel = Boolean(row.model) && Boolean(provider) && !availableModels.includes(row.model);
            return (
              <div
                key={index}
                className='rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 p-10px flex flex-col gap-8px'
              >
                <div className='flex items-center gap-8px flex-wrap'>
                  <Select
                    size='small'
                    style={{ width: 180 }}
                    placeholder={t('settings.moa.referenceProvider')}
                    value={row.provider_id || undefined}
                    onChange={(provider_id: ProviderId) => patchReference(index, { provider_id, model: '' })}
                  >
                    {enabledProviders.map((p) => (
                      <Select.Option key={p.id} value={p.id}>
                        {providerLabel(p)}
                      </Select.Option>
                    ))}
                  </Select>
                  <Select
                    size='small'
                    style={{ width: 220 }}
                    placeholder={t('settings.moa.referenceModel')}
                    value={row.model || undefined}
                    disabled={!provider}
                    onChange={(model: string) => patchReference(index, { model })}
                  >
                    {staleModel && (
                      <Select.Option key={row.model} value={row.model} disabled>
                        {row.model}
                      </Select.Option>
                    )}
                    {availableModels.map((m) => (
                      <Select.Option key={m} value={m}>
                        {m}
                      </Select.Option>
                    ))}
                  </Select>
                  <Button
                    size='small'
                    status='danger'
                    icon={<Delete theme='outline' size='14' />}
                    onClick={() => removeReference(index)}
                    aria-label={t('settings.moa.removeReference')}
                  />
                </div>
                <div className='flex items-center gap-12px flex-wrap'>
                  <div className='flex items-center gap-6px'>
                    <span className='text-12px text-t-secondary'>{t('settings.moa.referenceRowMaxTokens')}</span>
                    <InputNumber
                      size='small'
                      style={{ width: 120 }}
                      min={1}
                      step={1}
                      placeholder={t('settings.moa.optional')}
                      value={row.max_tokens}
                      onChange={(v) => patchReference(index, { max_tokens: typeof v === 'number' ? v : undefined })}
                    />
                  </div>
                  <div className='flex items-center gap-6px'>
                    <span className='text-12px text-t-secondary'>{t('settings.moa.referenceRowTemperature')}</span>
                    <InputNumber
                      size='small'
                      style={{ width: 120 }}
                      min={0}
                      max={2}
                      step={0.1}
                      placeholder={t('settings.moa.optional')}
                      value={row.temperature}
                      onChange={(v) => patchReference(index, { temperature: typeof v === 'number' ? v : undefined })}
                    />
                  </div>
                </div>
              </div>
            );
          })}

          <div>
            <Button size='small' icon={<Plus theme='outline' size='14' />} onClick={addReference}>
              {t('settings.moa.addReference')}
            </Button>
          </div>

          {/* Fan-out cadence */}
          <Typography.Text className='text-t-primary text-14px font-500 mt-4px'>
            {t('settings.moa.fanoutSection')}
          </Typography.Text>
          <div className='flex items-center gap-8px flex-wrap'>
            <Select
              size='small'
              style={{ width: 220 }}
              value={form.fanoutMode}
              onChange={(fanoutMode: FanoutMode) => patch({ fanoutMode })}
            >
              <Select.Option value='user_turn'>{t('settings.moa.fanoutUserTurn')}</Select.Option>
              <Select.Option value='per_iteration'>{t('settings.moa.fanoutPerIteration')}</Select.Option>
              <Select.Option value='every_n'>{t('settings.moa.fanoutEveryN')}</Select.Option>
            </Select>
            {form.fanoutMode === 'every_n' && (
              <div className='flex items-center gap-6px'>
                <span className='text-12px text-t-secondary'>{t('settings.moa.fanoutEveryNCount')}</span>
                <InputNumber
                  size='small'
                  style={{ width: 100 }}
                  min={1}
                  step={1}
                  value={form.everyN}
                  onChange={(v) => patch({ everyN: typeof v === 'number' ? Math.max(1, Math.round(v)) : 1 })}
                />
              </div>
            )}
          </div>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('settings.moa.fanoutHint')}
          </Typography.Paragraph>

          {/* Limits */}
          <div className='flex items-center gap-12px flex-wrap'>
            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('settings.moa.referenceTimeoutSecs')}</span>
              <InputNumber
                size='small'
                style={{ width: 160 }}
                min={1}
                step={1}
                placeholder='120'
                value={form.referenceTimeoutSecs}
                onChange={(v) => patch({ referenceTimeoutSecs: typeof v === 'number' ? v : undefined })}
              />
            </div>
            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('settings.moa.referenceMaxTokens')}</span>
              <InputNumber
                size='small'
                style={{ width: 160 }}
                min={1}
                step={1}
                placeholder='4096'
                value={form.referenceMaxTokens}
                onChange={(v) => patch({ referenceMaxTokens: typeof v === 'number' ? v : undefined })}
              />
            </div>
          </div>

          {/* Privacy filter */}
          <div className='flex items-center justify-between gap-12px'>
            <span className='text-t-primary text-14px'>{t('settings.moa.privacyFilter')}</span>
            <Select
              size='small'
              style={{ width: 220 }}
              value={form.privacyFilter}
              onChange={(privacyFilter: PrivacyFilter) => patch({ privacyFilter })}
            >
              <Select.Option value=''>{t('settings.moa.privacyOff')}</Select.Option>
              <Select.Option value='display'>{t('settings.moa.privacyDisplay')}</Select.Option>
              <Select.Option value='full'>{t('settings.moa.privacyFull')}</Select.Option>
            </Select>
          </div>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('settings.moa.privacyHint')}
          </Typography.Paragraph>

          {/* Trace */}
          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px'>{t('settings.moa.traceEnabled')}</span>
            <Switch checked={form.traceEnabled} onChange={(traceEnabled) => patch({ traceEnabled })} />
          </div>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('settings.moa.traceHint')}
          </Typography.Paragraph>

          <div className='flex flex-wrap gap-8px'>
            <Button type='primary' loading={saving || loading} onClick={save}>
              {t('common.save', { defaultValue: 'Save' })}
            </Button>
          </div>
        </div>
      </div>
    </SettingsPageWrapper>
  );
};

export default MoaSettings;
