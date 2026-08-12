

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Button,
  InputNumber,
  Message,
  Modal,
  Select,
  Spin,
  Switch,
  Table,
  Tag,
} from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/config/constants';
import type { IPoiSettings, IPoiStatusResponse, IPoiTopic } from '@/common/adapter/ipcBridge';
import { useModelProviderList } from '@/renderer/hooks/agent/useModelProviderList';
import {
  SettingsActionBar,
  SettingsGroup,
  SettingsPageHeader,
  SettingsPanel,
  SettingsList,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

/** Must match `POI_LLM_MODEL_FOLLOW_SESSION` in the Rust auxiliary provider. */
const FOLLOW_SESSION_MODEL = '__session__';

const TOPIC_STATUSES = ['candidate', 'active', 'rejected'] as const;

const PoiSettings: React.FC = () => {
  const { t } = useTranslation();
  const { providers, getAvailableModels, formatModelLabel } = useModelProviderList();
  const [settings, setSettings] = useState<IPoiSettings | null>(null);
  const [savedSettings, setSavedSettings] = useState<IPoiSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [status, setStatus] = useState<IPoiStatusResponse | null>(null);
  const [topics, setTopics] = useState<IPoiTopic[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [s, st, list] = await Promise.all([
        ipcBridge.poi.getSettings.invoke(),
        ipcBridge.poi.status.invoke(),
        ipcBridge.poi.listTopics.invoke(),
      ]);
      const normalizedSettings = {
        ...s,
        extractMode: s.extractMode || 'llm',
        autoExtractEnabled: s.autoExtractEnabled ?? true,
        autoExtractMinTurns: s.autoExtractMinTurns ?? 3,
        autoExtractMinUserChars: s.autoExtractMinUserChars ?? 50,
        autoExtractIdleSecs: s.autoExtractIdleSecs ?? 500,
        starterEnabled: s.starterEnabled ?? true,
      };
      setSettings(normalizedSettings);
      setSavedSettings(normalizedSettings);
      setStatus(st);
      setTopics(list.topics);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const flowyCloudProvider = useMemo(
    () => providers.find((p) => p.id === FLOWY_BUILTIN_PROVIDER_ID),
    [providers]
  );

  const availableCloudModels = useMemo(() => {
    if (!flowyCloudProvider) return [] as string[];
    return getAvailableModels(flowyCloudProvider);
  }, [flowyCloudProvider, getAvailableModels]);

  const firstAvailableModel = availableCloudModels[0] ?? '';

  const llmModelOptions = useMemo(() => {
    const options = [
      {
        label: t('poi.settings.llmModelFollowSession'),
        value: FOLLOW_SESSION_MODEL,
      },
    ];
    if (!flowyCloudProvider) {
      return options;
    }
    for (const model of availableCloudModels) {
      options.push({
        label: formatModelLabel(flowyCloudProvider, model),
        value: model,
      });
    }
    return options;
  }, [availableCloudModels, flowyCloudProvider, formatModelLabel, t]);

  const usesLlmExtract =
    settings?.extractMode === 'llm' || settings?.extractMode === 'hybrid';

  /** Unset config displays the first available model (product default). */
  const llmSelectValue = useMemo(() => {
    const trimmed = settings?.llmModel?.trim();
    if (trimmed === FOLLOW_SESSION_MODEL) return FOLLOW_SESSION_MODEL;
    if (trimmed) return trimmed;
    if (firstAvailableModel) return firstAvailableModel;
    return FOLLOW_SESSION_MODEL;
  }, [firstAvailableModel, settings?.llmModel]);

  const saveSettings = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      const llmModel =
        settings.llmModel?.trim() || firstAvailableModel || null;
      const saved = await ipcBridge.poi.updateSettings.invoke({
        ...settings,
        llmModel,
      });
      setSettings(saved);
      setSavedSettings(saved);
      setSaveError(null);
      void refresh();
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const isDirty = useMemo(
    () => Boolean(settings && savedSettings && JSON.stringify(settings) !== JSON.stringify(savedSettings)),
    [savedSettings, settings]
  );

  const handlePin = async (topic: IPoiTopic) => {
    try {
      await ipcBridge.poi.pinTopic.invoke({ id: topic.id, pinned: !topic.pinned });
      void refresh();
    } catch (e) {
      Message.error(String(e));
    }
  };

  const handleStatusChange = async (topic: IPoiTopic, next: string) => {
    try {
      await ipcBridge.poi.setTopicStatus.invoke({ id: topic.id, status: next });
      void refresh();
    } catch (e) {
      Message.error(String(e));
    }
  };

  const handleClearTopics = () => {
    Modal.confirm({
      title: t('poi.topics.clearConfirmTitle'),
      content: t('poi.topics.clearConfirmContent'),
      onOk: async () => {
        await ipcBridge.poi.clearTopics.invoke();
        Message.success(t('poi.topics.cleared'));
        void refresh();
      },
    });
  };

  const handleDeleteTopic = (topic: IPoiTopic) => {
    Modal.confirm({
      title: t('poi.topics.deleteConfirmTitle'),
      content: t('poi.topics.deleteConfirmContent'),
      onOk: async () => {
        try {
          await ipcBridge.poi.deleteTopic.invoke({ id: topic.id });
          Message.success(t('poi.topics.deleted'));
          void refresh();
        } catch (e) {
          Message.error(String(e));
        }
      },
    });
  };

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={t('poi.title')}
          description={t('poi.description')}
          meta={
            status && (
              <div className='flex flex-wrap gap-x-16px gap-y-6px text-12px text-t-secondary'>
                <span>{t('poi.status.topicCount', { count: status.topicCount })}</span>
                <span>{t('poi.status.extractMode', { mode: status.extractMode })}</span>
              </div>
            )
          }
          action={
            status && (
              <Tag color={status.enabled ? 'green' : 'gray'}>
                {status.enabled ? t('poi.status.enabled') : t('poi.status.disabled')}
              </Tag>
            )
          }
        />

        {settings ? (
          <>
            <SettingsGroup title={t('poi.settings.sectionBasics')}>
              <SettingsList>
                <PreferenceRow label={t('poi.settings.enabled')} controlLayout='compact'>
                  <Switch checked={settings.enabled} onChange={(v) => setSettings({ ...settings, enabled: v })} />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.maxTopics')} controlLayout='field'>
                  <InputNumber
                    className='w-140px'
                    min={1}
                    value={settings.maxTopics}
                    onChange={(v) => setSettings({ ...settings, maxTopics: Number(v) })}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.minTurnChars')} controlLayout='field'>
                  <InputNumber
                    className='w-140px'
                    min={0}
                    value={settings.minTurnChars}
                    onChange={(v) => setSettings({ ...settings, minTurnChars: Number(v) })}
                  />
                </PreferenceRow>
              </SettingsList>
            </SettingsGroup>

            <SettingsGroup title={t('poi.settings.sectionExtraction')}>
              <SettingsList>
                <PreferenceRow label={t('poi.settings.extractMode')} controlLayout='field'>
                  <Select
                    className='w-full sm:w-280px'
                    value={settings.extractMode}
                    onChange={(v) => setSettings({ ...settings, extractMode: v })}
                    options={[
                      { label: t('poi.settings.extractModeKeywords'), value: 'keywords' },
                      { label: t('poi.settings.extractModeLlm'), value: 'llm' },
                      { label: t('poi.settings.extractModeHybrid'), value: 'hybrid' },
                    ]}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.llmOnSessionEnd')} controlLayout='compact'>
                  <Switch
                    checked={settings.llmOnSessionEnd}
                    onChange={(v) => setSettings({ ...settings, llmOnSessionEnd: v })}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.llmModel')} description={t('poi.settings.llmModelHint')} controlLayout='field'>
                    <Select
                      className='w-full sm:w-280px'
                      value={llmSelectValue}
                      disabled={!usesLlmExtract}
                      onChange={(v) => setSettings({ ...settings, llmModel: v })}
                      options={llmModelOptions}
                    />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.perTurnBuffer')} controlLayout='compact'>
                  <Switch
                    checked={settings.perTurnBuffer}
                    onChange={(v) => setSettings({ ...settings, perTurnBuffer: v })}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('poi.settings.perTurnPersist')} controlLayout='compact'>
                  <Switch
                    checked={settings.perTurnPersist}
                    onChange={(v) => setSettings({ ...settings, perTurnPersist: v })}
                  />
                </PreferenceRow>
              </SettingsList>
            </SettingsGroup>

            <SettingsGroup title={t('poi.settings.sectionAutomatic')} description={t('poi.settings.autoExtractHint')}>
              <SettingsList>
                  <PreferenceRow label={t('poi.settings.autoExtractSection')} controlLayout='compact'>
                    <Switch
                      checked={settings.autoExtractEnabled}
                      onChange={(v) => setSettings({ ...settings, autoExtractEnabled: v })}
                    />
                  </PreferenceRow>
                        <PreferenceRow label={t('poi.settings.autoExtractMinTurns')} controlLayout='field'>
                          <InputNumber
                            className='w-140px'
                            min={1}
                            disabled={!settings.autoExtractEnabled}
                            value={settings.autoExtractMinTurns}
                            onChange={(v) => setSettings({ ...settings, autoExtractMinTurns: Number(v) })}
                          />
                        </PreferenceRow>
                        <PreferenceRow label={t('poi.settings.autoExtractMinUserChars')} controlLayout='field'>
                          <InputNumber
                            className='w-140px'
                            min={1}
                            disabled={!settings.autoExtractEnabled}
                            value={settings.autoExtractMinUserChars}
                            onChange={(v) => setSettings({ ...settings, autoExtractMinUserChars: Number(v) })}
                          />
                        </PreferenceRow>
                        <PreferenceRow
                          label={t('poi.settings.autoExtractIdleSecs')}
                          description={t('poi.settings.autoExtractIdleSecsHint')}
                          controlLayout='field'
                        >
                          <InputNumber
                            className='w-140px'
                            min={30}
                            disabled={!settings.autoExtractEnabled}
                            value={settings.autoExtractIdleSecs}
                            onChange={(v) => setSettings({ ...settings, autoExtractIdleSecs: Number(v) })}
                          />
                        </PreferenceRow>
              </SettingsList>
            </SettingsGroup>
            <SettingsActionBar
              visible={isDirty || Boolean(saveError)}
              saveLabel={t('common.save', { defaultValue: 'Save' })}
              onSave={() => void saveSettings()}
              resetLabel={t('common.cancel', { defaultValue: 'Cancel' })}
              onReset={() => {
                setSettings(savedSettings);
                setSaveError(null);
              }}
              loading={saving}
              error={saveError}
            />
          </>
        ) : (
          <div className='flex justify-center py-32px'>
            <Spin />
          </div>
        )}

        <SettingsGroup
          title={t('poi.topics.title')}
          action={
            <Button status='danger' size='small' disabled={topics.length === 0} onClick={handleClearTopics}>
              {t('poi.topics.clearAll')}
            </Button>
          }
        >
          <SettingsPanel>
            <div className='overflow-x-auto'>
              <Table
                loading={loading}
                data={topics}
                rowKey='id'
                pagination={{ pageSize: 10 }}
                columns={[
                  {
                    title: t('poi.topics.label'),
                    dataIndex: 'label',
                    render: (_, row) => (
                      <div className='min-w-0'>
                        <div className='font-500'>{row.label}</div>
                        {row.summary && <div className='max-w-280px truncate text-12px text-t-tertiary'>{row.summary}</div>}
                      </div>
                    ),
                  },
                  {
                    title: t('poi.topics.status'),
                    dataIndex: 'status',
                    width: 140,
                    render: (_, row) => (
                      <Select
                        size='small'
                        value={row.status}
                        onChange={(v) => handleStatusChange(row, v)}
                        options={TOPIC_STATUSES.map((s) => ({
                          label: t(`poi.topics.statuses.${s}`),
                          value: s,
                        }))}
                      />
                    ),
                  },
                  {
                    title: t('poi.topics.weight'),
                    dataIndex: 'weight',
                    width: 80,
                    render: (v) => Number(v).toFixed(2),
                  },
                  {
                    title: t('poi.topics.pinned'),
                    dataIndex: 'pinned',
                    width: 90,
                    render: (_, row) => (
                      <Switch size='small' checked={row.pinned} onChange={() => handlePin(row)} />
                    ),
                  },
                  {
                    title: t('poi.topics.actions'),
                    dataIndex: 'id',
                    width: 90,
                    render: (_, row) => (
                      <Button size='mini' status='danger' type='text' onClick={() => handleDeleteTopic(row)}>
                        {t('poi.topics.delete')}
                      </Button>
                    ),
                  },
                ]}
                noDataElement={t('poi.topics.empty')}
              />
            </div>
          </SettingsPanel>
        </SettingsGroup>
      </div>
    </SettingsPageWrapper>
  );
};

export default PoiSettings;
