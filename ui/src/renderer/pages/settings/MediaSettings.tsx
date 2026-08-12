

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Button,
  InputNumber,
  Message,
  Select,
  Spin,
  Switch,
  Table,
} from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IMediaSettings, IMediaWorkflowHistoryItem } from '@/common/adapter/ipcBridge';
import CreditsRefreshButton from '@/renderer/components/base/CreditsRefreshButton';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import {
  SettingsActionBar,
  SettingsGroup,
  SettingsPageHeader,
  SettingsPanel,
  SettingsList,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const MediaSettings: React.FC = () => {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<IMediaSettings | null>(null);
  const [savedSettings, setSavedSettings] = useState<IMediaSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const { balance, authenticated } = useCredits();
  const [history, setHistory] = useState<IMediaWorkflowHistoryItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const { imageModels, videoModels, revalidate: revalidateMediaModels } = useMediaModels();

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [s, h] = await Promise.all([
        ipcBridge.media.getSettings.invoke(),
        ipcBridge.media.workflowHistory.invoke({ limit: 50 }),
        revalidateMediaModels(),
      ]);
      setSettings(s);
      setSavedSettings(s);
      setHistory(h.runs);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, [revalidateMediaModels]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const imageModelOptions = useMemo(
    () =>
      imageModels.map((m) => ({
        label: m.name.trim() || formatCloudModelLabel(m.id),
        value: m.id,
      })),
    [imageModels]
  );
  const videoModelOptions = useMemo(
    () =>
      videoModels.map((m) => ({
        label: m.name.trim() || formatCloudModelLabel(m.id),
        value: m.id,
      })),
    [videoModels]
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
        video_default_aspect_ratio: settings.video_default_aspect_ratio,
        workflows_enabled: settings.workflows_enabled,
        workflows_max_retries: settings.workflows_max_retries,
      });
      setSettings(saved);
      setSavedSettings(saved);
      setSaveError(null);
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

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={t('media.title')}
          description={t('media.description')}
          meta={
            <div className='space-y-4px text-12px leading-18px text-t-tertiary'>
              <div className='tabular-nums'>
                {t('media.credits.balance', {
                  balance: authenticated ? balance : '—',
                })}
              </div>
              {settings && (
                <div>{settings.flowy_media_exposed ? t('media.agentHintReady') : t('media.agentHintLogin')}</div>
              )}
            </div>
          }
          action={
            <div className='flex items-center gap-8px'>
              <span
                className={`inline-flex items-center gap-5px rounded-full px-8px text-11px font-500 leading-16px ${
                  authenticated ? '' : 'bg-fill-2 text-t-tertiary'
                }`}
                style={
                  authenticated
                    ? {
                        backgroundColor: 'color-mix(in srgb, rgb(var(--primary-6)) 14%, transparent)',
                        color: 'rgb(var(--primary-6))',
                      }
                    : undefined
                }
              >
                {authenticated && (
                  <span
                    className='inline-block w-5px h-5px rounded-full'
                    style={{ backgroundColor: 'rgb(var(--primary-6))' }}
                  />
                )}
                {authenticated ? t('media.credits.authenticated') : t('media.credits.notAuthenticated')}
              </span>
              <CreditsRefreshButton size='sm' />
            </div>
          }
        />

        {settings ? (
          <>
            <SettingsList>
                <PreferenceRow label={t('media.settings.imageModel')}>
                  <Select
                    allowCreate
                    showSearch
                    className='w-full sm:w-280px'
                    value={settings.image_model || undefined}
                    onChange={(v) => setSettings({ ...settings, image_model: v })}
                    options={imageModelOptions}
                    placeholder={t('media.settings.selectModel')}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('media.settings.videoModel')}>
                  <Select
                    allowCreate
                    showSearch
                    className='w-full sm:w-280px'
                    value={settings.video_model || undefined}
                    onChange={(v) => setSettings({ ...settings, video_model: v })}
                    options={videoModelOptions}
                    placeholder={t('media.settings.selectModel')}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('media.settings.videoDuration')}>
                  <InputNumber
                    className='w-full sm:w-180px'
                    min={1}
                    max={60}
                    value={settings.video_default_duration}
                    onChange={(v) => setSettings({ ...settings, video_default_duration: Number(v) })}
                  />
                </PreferenceRow>
                <PreferenceRow
                  label={t('media.settings.videoAspectRatio', { defaultValue: '默认视频比例' })}
                >
                  <Select
                    className='w-full sm:w-180px'
                    value={settings.video_default_aspect_ratio || '16:9'}
                    onChange={(v) =>
                      setSettings({ ...settings, video_default_aspect_ratio: String(v) })
                    }
                    options={['16:9', '9:16', '1:1', '4:3', '3:4', '21:9'].map((value) => ({
                      label: value,
                      value,
                    }))}
                    getPopupContainer={() => document.body}
                    triggerProps={{ autoAlignPopupWidth: true }}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('media.settings.imageSaveLocally')}>
                  <Switch
                    checked={settings.image_save_locally}
                    onChange={(v) => setSettings({ ...settings, image_save_locally: v })}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('media.settings.videoSaveLocally')}>
                  <Switch
                    checked={settings.video_save_locally}
                    onChange={(v) => setSettings({ ...settings, video_save_locally: v })}
                  />
                </PreferenceRow>
                <PreferenceRow label={t('media.settings.workflowsEnabled')}>
                  <Switch
                    checked={settings.workflows_enabled}
                    onChange={(v) => setSettings({ ...settings, workflows_enabled: v })}
                  />
                </PreferenceRow>
            </SettingsList>
            <SettingsActionBar
              visible={isDirty || Boolean(saveError)}
              saveLabel={t('common.save', { defaultValue: 'Save' })}
              onSave={() => void save()}
              resetLabel={t('common.cancel', { defaultValue: 'Cancel' })}
              onReset={() => {
                setSettings(savedSettings);
                setSaveError(null);
              }}
              loading={saving || loading}
              error={saveError}
            />
          </>
        ) : (
          <div className='flex justify-center py-32px'>
            <Spin />
          </div>
        )}

        <SettingsGroup title={t('media.history.title')}>
          <SettingsPanel>
            <div className='overflow-x-auto'>
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
          </SettingsPanel>
        </SettingsGroup>
      </div>
    </SettingsPageWrapper>
  );
};

export default MediaSettings;
