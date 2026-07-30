

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, InputNumber, Message, Modal, Spin, Switch } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IInsightsContributionStatus } from '@/common/adapter/ipcBridge';
import {
  SettingsGroup,
  SettingsNestedRows,
  SettingsPageHeader,
  SettingsPanel,
  SettingsPanelFooter,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const InsightsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<IInsightsContributionStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [onSessionEnd, setOnSessionEnd] = useState(true);
  const [autoExtractEnabled, setAutoExtractEnabled] = useState(true);
  const [autoExtractIdleSecs, setAutoExtractIdleSecs] = useState(300);
  const [skillMiningEnabled, setSkillMiningEnabled] = useState(false);
  const [redactedBody, setRedactedBody] = useState(true);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [flushing, setFlushing] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const s = await ipcBridge.insights.getStatus.invoke();
      setStatus(s);
      setEnabled(s.enabled);
      setOnSessionEnd(s.on_session_end);
      setAutoExtractEnabled(s.auto_extract_enabled ?? true);
      setAutoExtractIdleSecs(s.auto_extract_idle_secs ?? 300);
      setSkillMiningEnabled(s.skill_mining_enabled ?? false);
      setRedactedBody(s.redacted_body);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    setSaving(true);
    try {
      const saved = await ipcBridge.insights.updateContribution.invoke({
        enabled,
        on_session_end: onSessionEnd,
        auto_extract_enabled: autoExtractEnabled,
        auto_extract_idle_secs: autoExtractIdleSecs,
        skill_mining_enabled: skillMiningEnabled,
        redacted_body: redactedBody,
      });
      setStatus(saved);
      Message.success(t('insights.settings.saved'));
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const flush = async () => {
    setFlushing(true);
    try {
      const result = await ipcBridge.insights.flushContribution.invoke();
      Message.success(
        t('insights.actions.flushSuccess', {
          uploaded: result.uploaded,
          duplicates: result.duplicates,
          rejected: result.rejected,
        })
      );
      void refresh();
    } catch (e) {
      Message.error(String(e));
    } finally {
      setFlushing(false);
    }
  };

  const resetOutbox = (clearAll: boolean) => {
    Modal.confirm({
      title: clearAll ? t('insights.actions.resetAllTitle') : t('insights.actions.resetFailedTitle'),
      content: clearAll ? t('insights.actions.resetAllContent') : t('insights.actions.resetFailedContent'),
      onOk: async () => {
        const result = await ipcBridge.insights.resetOutbox.invoke({ clear_all: clearAll });
        Message.success(t('insights.actions.resetSuccess', { count: result.affected }));
        void refresh();
      },
    });
  };

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={t('insights.title')}
          description={t('insights.description')}
          meta={<span className='text-12px text-t-tertiary'>{t('insights.settings.serverManagedHint')}</span>}
        />

        <SettingsPanel>
          {status ? (
            <>
              <div className='w-full flex flex-col divide-y divide-border-2'>
                <PreferenceRow label={t('insights.settings.enabled')}>
                  <Switch checked={enabled} onChange={setEnabled} />
                </PreferenceRow>
                <PreferenceRow label={t('insights.settings.onSessionEnd')}>
                  <Switch checked={onSessionEnd} onChange={setOnSessionEnd} />
                </PreferenceRow>
                <PreferenceRow label={t('insights.settings.redactedBody')}>
                  <Switch checked={redactedBody} onChange={setRedactedBody} />
                </PreferenceRow>
                <div>
                  <PreferenceRow
                    label={t('insights.settings.autoExtractSection')}
                    description={t('insights.settings.autoExtractHint')}
                  >
                    <Switch checked={autoExtractEnabled} onChange={setAutoExtractEnabled} />
                  </PreferenceRow>
                  {autoExtractEnabled && (
                    <SettingsNestedRows>
                      <PreferenceRow
                        label={t('insights.settings.autoExtractIdleSecs')}
                        description={`${t('insights.settings.autoExtractIdleSecsHint')} ${t(
                          'insights.settings.minWorkTurnsHint',
                          { count: status.min_work_turns }
                        )}`}
                      >
                        <InputNumber
                          className='w-full sm:w-180px'
                          min={30}
                          value={autoExtractIdleSecs}
                          onChange={(v) => setAutoExtractIdleSecs(Number(v))}
                        />
                      </PreferenceRow>
                    </SettingsNestedRows>
                  )}
                </div>
                <PreferenceRow
                  label={t('insights.settings.skillMiningEnabled')}
                  description={t('insights.settings.skillMiningHint')}
                >
                  <Switch checked={skillMiningEnabled} onChange={setSkillMiningEnabled} />
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

        <SettingsGroup
          title={t('insights.actions.outboxTitle')}
          description={t('insights.actions.outboxDescription')}
        >
          <SettingsPanel>
            {status ? (
              <div className='w-full flex flex-col divide-y divide-border-2'>
                <PreferenceRow label={t('insights.status.uploadReady')}>
                  <span className={status.upload_ready ? 'text-success-6' : 'text-t-tertiary'}>
                    {status.upload_ready ? t('common.yes', { defaultValue: 'Yes' }) : t('common.no', { defaultValue: 'No' })}
                  </span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.authConfigured')}>
                  <span className={status.auth_configured ? 'text-success-6' : 'text-t-tertiary'}>
                    {status.auth_configured ? t('common.yes', { defaultValue: 'Yes' }) : t('common.no', { defaultValue: 'No' })}
                  </span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.outboxPending')}>
                  <span className='tabular-nums text-t-primary'>{status.outbox_pending}</span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.outboxFailed')}>
                  <span className={status.outbox_failed > 0 ? 'tabular-nums text-danger-6' : 'tabular-nums text-t-primary'}>
                    {status.outbox_failed}
                  </span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.outboxSent')}>
                  <span className='tabular-nums text-t-primary'>{status.outbox_sent}</span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.endpoint')}>
                  <span className='max-w-300px break-all text-right text-12px text-t-secondary'>
                    {status.endpoint || t('insights.status.endpointPending')}
                  </span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.installationId')}>
                  <span className='max-w-300px break-all text-right text-12px text-t-secondary'>
                    {status.installation_id}
                  </span>
                </PreferenceRow>
              </div>
            ) : (
              <div className='flex justify-center py-24px'>
                <Spin />
              </div>
            )}
            <SettingsPanelFooter className='flex-wrap gap-8px'>
              <Button loading={flushing} onClick={flush}>
                {t('insights.actions.flush')}
              </Button>
              <Button onClick={() => resetOutbox(false)}>{t('insights.actions.resetFailed')}</Button>
              <Button status='danger' onClick={() => resetOutbox(true)}>
                {t('insights.actions.resetAll')}
              </Button>
            </SettingsPanelFooter>
          </SettingsPanel>
        </SettingsGroup>
      </div>
    </SettingsPageWrapper>
  );
};

export default InsightsSettings;
