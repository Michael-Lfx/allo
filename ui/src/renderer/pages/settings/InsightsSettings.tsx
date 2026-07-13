/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Descriptions, InputNumber, Message, Modal, Switch, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { IInsightsContributionStatus } from '@/common/adapter/ipcBridge';
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
      <div className='flex flex-col gap-20px max-w-640px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('insights.title')}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('insights.description')}
          </Typography.Paragraph>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px mt-8px'>
            {t('insights.settings.serverManagedHint')}
          </Typography.Paragraph>
        </div>

        {status && (
          <Descriptions
            column={1}
            size='small'
            data={[
              {
                label: t('insights.status.uploadReady'),
                value: status.upload_ready ? t('common.yes', { defaultValue: 'Yes' }) : t('common.no', { defaultValue: 'No' }),
              },
              {
                label: t('insights.status.authConfigured'),
                value: status.auth_configured ? t('common.yes', { defaultValue: 'Yes' }) : t('common.no', { defaultValue: 'No' }),
              },
              { label: t('insights.status.endpoint'), value: status.endpoint || t('insights.status.endpointPending') },
              { label: t('insights.status.outboxPending'), value: String(status.outbox_pending) },
              { label: t('insights.status.outboxFailed'), value: String(status.outbox_failed) },
              { label: t('insights.status.outboxSent'), value: String(status.outbox_sent) },
              { label: t('insights.status.installationId'), value: status.installation_id },
            ]}
          />
        )}

        <div className='flex flex-col gap-14px'>
          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px font-500'>{t('insights.settings.enabled')}</span>
            <Switch checked={enabled} onChange={setEnabled} />
          </div>

          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px'>{t('insights.settings.onSessionEnd')}</span>
            <Switch checked={onSessionEnd} onChange={setOnSessionEnd} />
          </div>

          <Typography.Text className='text-t-primary text-14px font-500 mt-4px'>
            {t('insights.settings.autoExtractSection')}
          </Typography.Text>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('insights.settings.autoExtractHint')}
          </Typography.Paragraph>

          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px'>{t('insights.settings.autoExtractEnabled')}</span>
            <Switch checked={autoExtractEnabled} onChange={setAutoExtractEnabled} />
          </div>

          {autoExtractEnabled && (
            <div className='flex flex-col gap-6px max-w-320px'>
              <span className='text-t-secondary text-13px'>{t('insights.settings.autoExtractIdleSecs')}</span>
              <InputNumber
                min={30}
                value={autoExtractIdleSecs}
                onChange={(v) => setAutoExtractIdleSecs(Number(v))}
              />
              <span className='text-12px text-t-tertiary'>{t('insights.settings.autoExtractIdleSecsHint')}</span>
              {status && (
                <span className='text-12px text-t-tertiary'>
                  {t('insights.settings.minWorkTurnsHint', { count: status.min_work_turns })}
                </span>
              )}
            </div>
          )}

          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px'>{t('insights.settings.skillMiningEnabled', { defaultValue: '技能挖矿' })}</span>
            <Switch checked={skillMiningEnabled} onChange={setSkillMiningEnabled} />
          </div>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-12px'>
            {t('insights.settings.skillMiningHint', { defaultValue: '开启后，普通对话中的工具调用模式将被自动提取为技能建议。' })}
          </Typography.Paragraph>

          <div className='flex items-center justify-between'>
            <span className='text-t-primary text-14px'>{t('insights.settings.redactedBody')}</span>
            <Switch checked={redactedBody} onChange={setRedactedBody} />
          </div>

          <div className='flex flex-wrap gap-8px'>
            <Button type='primary' loading={saving || loading} onClick={save}>
              {t('common.save', { defaultValue: 'Save' })}
            </Button>
            <Button loading={flushing} onClick={flush}>
              {t('insights.actions.flush')}
            </Button>
            <Button onClick={() => resetOutbox(false)}>{t('insights.actions.resetFailed')}</Button>
            <Button status='danger' onClick={() => resetOutbox(true)}>
              {t('insights.actions.resetAll')}
            </Button>
          </div>
        </div>
      </div>
    </SettingsPageWrapper>
  );
};

export default InsightsSettings;
