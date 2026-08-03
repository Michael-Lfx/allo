

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
  Tag,
} from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IMediaSettings, IMediaWorkflowHistoryItem } from '@/common/adapter/ipcBridge';
import { Refresh } from '@icon-park/react';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import {
  SettingsGroup,
  SettingsPageHeader,
  SettingsPanel,
  SettingsPanelFooter,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const MediaSettings: React.FC = () => {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<IMediaSettings | null>(null);
  const {
    balance,
    authenticated,
    isFetchingBalance,
    cooldownSeconds,
    canRefresh,
    manualRefresh,
  } = useCredits();
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
      imageModels.map((id) => ({
        label: formatCloudModelLabel(id),
        value: id,
      })),
    [imageModels]
  );
  const videoModelOptions = useMemo(
    () =>
      videoModels.map((id) => ({
        label: formatCloudModelLabel(id),
        value: id,
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
      Message.success(t('media.settings.saved'));
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

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
              <Tag color={authenticated ? 'green' : 'gray'}>
                {authenticated ? t('media.credits.authenticated') : t('media.credits.notAuthenticated')}
              </Tag>
              <Button
                size='mini'
                type='text'
                disabled={!canRefresh}
                onClick={manualRefresh}
                aria-label={t('common.userMenu.refreshCredits', { defaultValue: '刷新积分余额' })}
                className='flex items-center justify-center text-t-tertiary'
              >
                {cooldownSeconds > 0 ? (
                  <span className='text-12px tabular-nums'>{cooldownSeconds}s</span>
                ) : (
                  <Refresh
                    theme='outline'
                    size='14'
                    fill='currentColor'
                    className={isFetchingBalance ? 'animate-spin' : ''}
                  />
                )}
              </Button>
            </div>
          }
        />

        <SettingsPanel>
          {settings ? (
            <>
              <div className='w-full flex flex-col divide-y divide-border-2'>
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
              </div>
              <SettingsPanelFooter className='justify-end'>
                <Button type='primary' loading={saving || loading} onClick={save}>
                  {t('common.save', { defaultValue: 'Save' })}
                </Button>
              </SettingsPanelFooter>
            </>
          ) : (
            <div className='flex justify-center py-32px'>
              <Spin />
            </div>
          )}
        </SettingsPanel>

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
