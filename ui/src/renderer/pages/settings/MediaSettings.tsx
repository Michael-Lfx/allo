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
  Select,
  Switch,
  Table,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IMediaCredits, IMediaModelList, IMediaSettings, IMediaWorkflowHistoryItem } from '@/common/adapter/ipcBridge';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const MediaSettings: React.FC = () => {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<IMediaSettings | null>(null);
  const [credits, setCredits] = useState<IMediaCredits | null>(null);
  const [models, setModels] = useState<IMediaModelList | null>(null);
  const [history, setHistory] = useState<IMediaWorkflowHistoryItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [s, c, m, h] = await Promise.all([
        ipcBridge.media.getSettings.invoke(),
        ipcBridge.media.getCredits.invoke(),
        ipcBridge.media.listModels.invoke(),
        ipcBridge.media.workflowHistory.invoke({ limit: 50 }),
      ]);
      setSettings(s);
      setCredits(c);
      setModels(m);
      setHistory(h.runs);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const imageModelOptions = useMemo(
    () =>
      (models?.image_models ?? []).map((id) => ({
        label: formatCloudModelLabel(id),
        value: id,
      })),
    [models]
  );
  const videoModelOptions = useMemo(
    () =>
      (models?.video_models ?? []).map((id) => ({
        label: formatCloudModelLabel(id),
        value: id,
      })),
    [models]
  );

  const save = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      const saved = await ipcBridge.media.updateSettings.invoke({
        image_model: settings.image_model,
        video_model: settings.video_model,
        image_save_locally: settings.image_save_locally,
        video_save_locally: settings.video_save_locally,
        video_default_duration: settings.video_default_duration,
        workflows_enabled: settings.workflows_enabled,
        workflows_max_retries: settings.workflows_max_retries,
      });
      setSettings(saved);
      Message.success(t('media.settings.saved'));
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingsPageWrapper>
      <div className='flex flex-col gap-20px max-w-960px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('media.title')}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('media.description')}
          </Typography.Paragraph>
        </div>

        {credits && (
          <div className='flex flex-wrap items-center gap-12px text-13px'>
            <Tag color={credits.authenticated ? 'green' : 'gray'}>
              {credits.authenticated ? t('media.credits.authenticated') : t('media.credits.notAuthenticated')}
            </Tag>
            <span>{t('media.credits.balance', { balance: credits.balance })}</span>
          </div>
        )}

        {settings && !settings.flowy_media_exposed && (
          <Typography.Paragraph className='!mb-0 text-t-secondary text-13px'>
            {t('media.agentHintLogin')}
          </Typography.Paragraph>
        )}

        {settings && settings.flowy_media_exposed && (
          <Typography.Paragraph className='!mb-0 text-t-secondary text-13px'>
            {t('media.agentHintReady')}
          </Typography.Paragraph>
        )}

        {settings && (
          <div className='flex flex-col gap-14px max-w-640px'>
            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('media.settings.imageModel')}</span>
              <Select
                allowCreate
                showSearch
                value={settings.image_model || undefined}
                onChange={(v) => setSettings({ ...settings, image_model: v })}
                options={imageModelOptions}
                placeholder={t('media.settings.selectModel')}
              />
            </div>

            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('media.settings.videoModel')}</span>
              <Select
                allowCreate
                showSearch
                value={settings.video_model || undefined}
                onChange={(v) => setSettings({ ...settings, video_model: v })}
                options={videoModelOptions}
                placeholder={t('media.settings.selectModel')}
              />
            </div>

            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('media.settings.videoDuration')}</span>
              <InputNumber
                min={1}
                max={60}
                value={settings.video_default_duration}
                onChange={(v) => setSettings({ ...settings, video_default_duration: Number(v) })}
              />
            </div>

            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('media.settings.imageSaveLocally')}</span>
              <Switch
                checked={settings.image_save_locally}
                onChange={(v) => setSettings({ ...settings, image_save_locally: v })}
              />
            </div>
            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('media.settings.videoSaveLocally')}</span>
              <Switch
                checked={settings.video_save_locally}
                onChange={(v) => setSettings({ ...settings, video_save_locally: v })}
              />
            </div>
            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px'>{t('media.settings.workflowsEnabled')}</span>
              <Switch
                checked={settings.workflows_enabled}
                onChange={(v) => setSettings({ ...settings, workflows_enabled: v })}
              />
            </div>

            <div>
              <Button type='primary' loading={saving || loading} onClick={save}>
                {t('common.save', { defaultValue: 'Save' })}
              </Button>
            </div>
          </div>
        )}

        <Divider />

        <Typography.Title heading={6} className='!m-0'>
          {t('media.history.title')}
        </Typography.Title>

        <Table
          loading={loading}
          data={history}
          rowKey='run_id'
          pagination={{ pageSize: 8 }}
          columns={[
            { title: t('media.history.workflow'), dataIndex: 'workflow_id' },
            { title: t('media.history.status'), dataIndex: 'status', width: 120 },
            {
              title: t('media.history.step'),
              dataIndex: 'current_step',
              render: (v) => v ?? '—',
            },
            {
              title: t('media.history.error'),
              dataIndex: 'error',
              render: (v) => (v ? <span className='text-danger-6 text-12px'>{v}</span> : '—'),
            },
          ]}
          noDataElement={t('media.history.empty')}
        />
      </div>
    </SettingsPageWrapper>
  );
};

export default MediaSettings;
