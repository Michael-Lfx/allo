

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, InputNumber, Modal, Spin, Switch } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import type { IInsightsContributionStatus } from '@/common/adapter/ipcBridge';
import {
  SettingsActionBar,
  SettingsControlGroup,
  SettingsGroup,
  SettingsList,
  SettingsNestedRows,
  SettingsPageHeader,
  SettingsPanel,
  SettingsPanelFooter,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

type InsightsDraft = {
  enabled: boolean;
  onSessionEnd: boolean;
  autoExtractEnabled: boolean;
  autoExtractIdleSecs: number;
  skillMiningEnabled: boolean;
  redactedBody: boolean;
};

const InsightsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<IInsightsContributionStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [onSessionEnd, setOnSessionEnd] = useState(true);
  const [autoExtractEnabled, setAutoExtractEnabled] = useState(true);
  const [autoExtractIdleSecs, setAutoExtractIdleSecs] = useState(300);
  const [skillMiningEnabled, setSkillMiningEnabled] = useState(false);
  const [redactedBody, setRedactedBody] = useState(true);
  const [savedDraft, setSavedDraft] = useState<InsightsDraft | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
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
      setSavedDraft({
        enabled: s.enabled,
        onSessionEnd: s.on_session_end,
        autoExtractEnabled: s.auto_extract_enabled ?? true,
        autoExtractIdleSecs: s.auto_extract_idle_secs ?? 300,
        skillMiningEnabled: s.skill_mining_enabled ?? false,
        redactedBody: s.redacted_body,
      });
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
    const draft: InsightsDraft = {
      enabled,
      onSessionEnd,
      autoExtractEnabled,
      autoExtractIdleSecs,
      skillMiningEnabled,
      redactedBody,
    };
    setSaving(true);
    try {
      const saved = await ipcBridge.insights.updateContribution.invoke({
        enabled: draft.enabled,
        on_session_end: draft.onSessionEnd,
        auto_extract_enabled: draft.autoExtractEnabled,
        auto_extract_idle_secs: draft.autoExtractIdleSecs,
        skill_mining_enabled: draft.skillMiningEnabled,
        redacted_body: draft.redactedBody,
      });
      setStatus(saved);
      setSavedDraft(draft);
      setSaveError(null);
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const hasUnsavedChanges = Boolean(
    savedDraft &&
      JSON.stringify(savedDraft) !==
        JSON.stringify({
          enabled,
          onSessionEnd,
          autoExtractEnabled,
          autoExtractIdleSecs,
          skillMiningEnabled,
          redactedBody,
        })
  );

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

        {status ? (
          <>
            <SettingsList>
                <PreferenceRow label={t('insights.settings.enabled')} controlLayout='compact'>
                  <Switch checked={enabled} onChange={setEnabled} />
                </PreferenceRow>
                <PreferenceRow label={t('insights.settings.onSessionEnd')} controlLayout='compact'>
                  <Switch checked={onSessionEnd} onChange={setOnSessionEnd} />
                </PreferenceRow>
                <PreferenceRow label={t('insights.settings.redactedBody')} controlLayout='compact'>
                  <Switch checked={redactedBody} onChange={setRedactedBody} />
                </PreferenceRow>
                <div>
                  <PreferenceRow
                    label={t('insights.settings.autoExtractSection')}
                    description={t('insights.settings.autoExtractHint')}
                    controlLayout='compact'
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
                      controlLayout='field'
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
                  controlLayout='compact'
                >
                  <Switch checked={skillMiningEnabled} onChange={setSkillMiningEnabled} />
                </PreferenceRow>
            </SettingsList>
            <SettingsActionBar
              visible={hasUnsavedChanges || Boolean(saveError)}
              saveLabel={t('common.save', { defaultValue: 'Save' })}
              onSave={() => void save()}
              resetLabel={t('common.cancel', { defaultValue: 'Cancel' })}
              onReset={() => {
                if (savedDraft) {
                  setEnabled(savedDraft.enabled);
                  setOnSessionEnd(savedDraft.onSessionEnd);
                  setAutoExtractEnabled(savedDraft.autoExtractEnabled);
                  setAutoExtractIdleSecs(savedDraft.autoExtractIdleSecs);
                  setSkillMiningEnabled(savedDraft.skillMiningEnabled);
                  setRedactedBody(savedDraft.redactedBody);
                }
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

        <SettingsGroup
          title={t('insights.actions.outboxTitle')}
          description={t('insights.actions.outboxDescription')}
        >
          <SettingsPanel>
            {status ? (
              <div className='w-full flex flex-col divide-y divide-border-2'>
                <PreferenceRow label={t('insights.status.uploadReady')}>
                  <span className={status.upload_ready ? 'text-success-6' : 'text-t-tertiary'}>
                    {status.upload_ready ? t('common.yes') : t('common.no')}
                  </span>
                </PreferenceRow>
                <PreferenceRow label={t('insights.status.authConfigured')}>
                  <span className={status.auth_configured ? 'text-success-6' : 'text-t-tertiary'}>
                    {status.auth_configured ? t('common.yes') : t('common.no')}
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
            <SettingsPanelFooter className='pt-12px'>
              <SettingsControlGroup className='justify-start'>
              <Button loading={flushing} onClick={flush}>
                {t('insights.actions.flush')}
              </Button>
              <Button onClick={() => resetOutbox(false)}>{t('insights.actions.resetFailed')}</Button>
              <Button status='danger' onClick={() => resetOutbox(true)}>
                {t('insights.actions.resetAll')}
              </Button>
              </SettingsControlGroup>
            </SettingsPanelFooter>
          </SettingsPanel>
        </SettingsGroup>
      </div>
    </SettingsPageWrapper>
  );
};

export default InsightsSettings;
