/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Divider,
  InputNumber,
  Message,
  Modal,
  Select,
  Switch,
  Table,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/config/constants';
import type { IPoiSettings, IPoiStatusResponse, IPoiTopic } from '@/common/adapter/ipcBridge';
import { useModelProviderList } from '@/renderer/hooks/agent/useModelProviderList';
import SettingsPageWrapper from './components/SettingsPageWrapper';

/** Must match `POI_LLM_MODEL_FOLLOW_SESSION` in the Rust auxiliary provider. */
const FOLLOW_SESSION_MODEL = '__session__';

const TOPIC_STATUSES = ['candidate', 'active', 'rejected'] as const;

const PoiSettings: React.FC = () => {
  const { t } = useTranslation();
  const { providers, getAvailableModels, formatModelLabel } = useModelProviderList();
  const [settings, setSettings] = useState<IPoiSettings | null>(null);
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
      setSettings({
        ...s,
        extractMode: s.extractMode || 'llm',
        autoExtractEnabled: s.autoExtractEnabled ?? true,
        autoExtractMinTurns: s.autoExtractMinTurns ?? 3,
        autoExtractMinUserChars: s.autoExtractMinUserChars ?? 50,
        autoExtractIdleSecs: s.autoExtractIdleSecs ?? 500,
        starterEnabled: s.starterEnabled ?? true,
      });
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
      Message.success(t('poi.settings.saved'));
      void refresh();
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

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
      <div className='flex flex-col gap-20px max-w-960px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('poi.title')}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('poi.description')}
          </Typography.Paragraph>
        </div>

        {status && (
          <div className='flex flex-wrap gap-12px text-13px text-t-secondary'>
            <Tag color={status.enabled ? 'green' : 'gray'}>
              {status.enabled ? t('poi.status.enabled') : t('poi.status.disabled')}
            </Tag>
            <span>
              {t('poi.status.topicCount', { count: status.topicCount })}
            </span>
            <span>{t('poi.status.extractMode', { mode: status.extractMode })}</span>
          </div>
        )}

        {settings && (
          <div className='flex flex-col gap-14px max-w-640px'>
            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px font-500'>{t('poi.settings.enabled')}</span>
              <Switch checked={settings.enabled} onChange={(v) => setSettings({ ...settings, enabled: v })} />
            </div>

            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('poi.settings.extractMode')}</span>
              <Select
                value={settings.extractMode}
                onChange={(v) => setSettings({ ...settings, extractMode: v })}
                options={[
                  { label: t('poi.settings.extractModeKeywords'), value: 'keywords' },
                  { label: t('poi.settings.extractModeLlm'), value: 'llm' },
                  { label: t('poi.settings.extractModeHybrid'), value: 'hybrid' },
                ]}
              />
            </div>

            <div className='grid grid-cols-1 md:grid-cols-2 gap-12px'>
              <div className='flex flex-col gap-6px'>
                <span className='text-t-secondary text-13px'>{t('poi.settings.maxTopics')}</span>
                <InputNumber
                  min={1}
                  value={settings.maxTopics}
                  onChange={(v) => setSettings({ ...settings, maxTopics: Number(v) })}
                />
              </div>
              <div className='flex flex-col gap-6px'>
                <span className='text-t-secondary text-13px'>{t('poi.settings.minTurnChars')}</span>
                <InputNumber
                  min={0}
                  value={settings.minTurnChars}
                  onChange={(v) => setSettings({ ...settings, minTurnChars: Number(v) })}
                />
              </div>
            </div>

            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('poi.settings.llmOnSessionEnd')}</span>
              <Switch
                checked={settings.llmOnSessionEnd}
                onChange={(v) => setSettings({ ...settings, llmOnSessionEnd: v })}
              />
            </div>

            {usesLlmExtract && (
              <div className='flex flex-col gap-6px'>
                <span className='text-t-secondary text-13px'>{t('poi.settings.llmModel')}</span>
                <Select
                  value={llmSelectValue}
                  onChange={(v) =>
                    setSettings({
                      ...settings,
                      llmModel: v,
                    })
                  }
                  options={llmModelOptions}
                />
                <span className='text-12px text-t-tertiary'>{t('poi.settings.llmModelHint')}</span>
              </div>
            )}

            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('poi.settings.perTurnBuffer')}</span>
              <Switch
                checked={settings.perTurnBuffer}
                onChange={(v) => setSettings({ ...settings, perTurnBuffer: v })}
              />
            </div>
            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('poi.settings.perTurnPersist')}</span>
              <Switch
                checked={settings.perTurnPersist}
                onChange={(v) => setSettings({ ...settings, perTurnPersist: v })}
              />
            </div>

            <Divider className='!my-4px' />

            <Typography.Text className='text-t-primary text-14px font-500'>
              {t('poi.settings.autoExtractSection')}
            </Typography.Text>
            <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
              {t('poi.settings.autoExtractHint')}
            </Typography.Paragraph>

            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('poi.settings.autoExtractEnabled')}</span>
              <Switch
                checked={settings.autoExtractEnabled}
                onChange={(v) => setSettings({ ...settings, autoExtractEnabled: v })}
              />
            </div>

            {settings.autoExtractEnabled && (
              <div className='grid grid-cols-1 md:grid-cols-2 gap-12px'>
                <div className='flex flex-col gap-6px'>
                  <span className='text-t-secondary text-13px'>{t('poi.settings.autoExtractMinTurns')}</span>
                  <InputNumber
                    min={1}
                    value={settings.autoExtractMinTurns}
                    onChange={(v) => setSettings({ ...settings, autoExtractMinTurns: Number(v) })}
                  />
                </div>
                <div className='flex flex-col gap-6px'>
                  <span className='text-t-secondary text-13px'>{t('poi.settings.autoExtractMinUserChars')}</span>
                  <InputNumber
                    min={1}
                    value={settings.autoExtractMinUserChars}
                    onChange={(v) => setSettings({ ...settings, autoExtractMinUserChars: Number(v) })}
                  />
                </div>
                <div className='flex flex-col gap-6px md:col-span-2'>
                  <span className='text-t-secondary text-13px'>{t('poi.settings.autoExtractIdleSecs')}</span>
                  <InputNumber
                    min={30}
                    value={settings.autoExtractIdleSecs}
                    onChange={(v) => setSettings({ ...settings, autoExtractIdleSecs: Number(v) })}
                  />
                  <span className='text-12px text-t-tertiary'>{t('poi.settings.autoExtractIdleSecsHint')}</span>
                </div>
              </div>
            )}

            <div>
              <Button type='primary' loading={saving} onClick={saveSettings}>
                {t('common.save', { defaultValue: 'Save' })}
              </Button>
            </div>
          </div>
        )}

        <Divider />

        <div className='flex items-center justify-between'>
          <Typography.Title heading={6} className='!m-0'>
            {t('poi.topics.title')}
          </Typography.Title>
          <Button status='danger' size='small' disabled={topics.length === 0} onClick={handleClearTopics}>
            {t('poi.topics.clearAll')}
          </Button>
        </div>

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
                <div>
                  <div className='font-500'>{row.label}</div>
                  {row.summary && <div className='text-12px text-t-tertiary truncate max-w-280px'>{row.summary}</div>}
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
    </SettingsPageWrapper>
  );
};

export default PoiSettings;
