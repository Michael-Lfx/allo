/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Popconfirm } from '@arco-design/web-react';
import type { IPresentationCapability } from '@/common/adapter/ipcBridge';
import PresentationInstallProgress from '@/renderer/components/presentation/PresentationInstallProgress';
import {
  SettingsControlGroup,
  SettingsGroup,
  SettingsList,
  SettingsPageHeader,
  SettingsRow,
  SettingsStatus,
  type SettingsStatusTone,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import { formatBytes, usePresentationCapability } from '@/renderer/hooks/presentation/usePresentationCapability';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const STATUS_TONE: Record<IPresentationCapability['status'], SettingsStatusTone> = {
  not_installed: 'neutral',
  installing: 'warning',
  ready: 'success',
  failed: 'error',
};

function formatTime(ms: number | undefined): string {
  if (!ms) return '—';
  return new Date(ms).toLocaleString();
}

/**
 * Settings → Capabilities → Presentations: the host-managed ppt-master Skill.
 * Nomi installs it on demand; this page shows the same install and offers
 * repair / cancel / uninstall.
 */
const PresentationSettings: React.FC = () => {
  const { t } = useTranslation();
  const { capability, loading, error, pending, install, repair, cancel, uninstall } = usePresentationCapability();
  const status = capability?.status;
  const installing = status === 'installing';

  const actions = capability ? (
    <SettingsControlGroup>
      {(status === 'not_installed' || status === 'failed') && (
        <Button type='primary' loading={pending === 'install'} onClick={() => void install()}>
          {status === 'failed' ? t('presentation.actions.retry') : t('presentation.actions.install')}
        </Button>
      )}
      {installing && (
        <Button loading={pending === 'cancel'} onClick={() => void cancel()}>
          {t('presentation.actions.cancel')}
        </Button>
      )}
      {status === 'ready' && (
        <Button loading={pending === 'repair'} onClick={() => void repair()}>
          {t('presentation.actions.repair')}
        </Button>
      )}
      {(status === 'ready' || status === 'failed') && (
        <Popconfirm
          title={t('presentation.actions.uninstallConfirm')}
          onOk={() => void uninstall()}
          okText={t('presentation.actions.uninstall')}
          cancelText={t('common.cancel', { defaultValue: 'Cancel' })}
        >
          <Button status='danger' loading={pending === 'uninstall'}>
            {t('presentation.actions.uninstall')}
          </Button>
        </Popconfirm>
      )}
    </SettingsControlGroup>
  ) : null;

  return (
    <SettingsPageWrapper layout='form' loading={loading && capability === null}>
      <div className='space-y-24px'>
        <SettingsPageHeader title={t('presentation.settings.title')} description={t('presentation.settings.description')} />

        {capability && (
          <SettingsGroup title={t('presentation.settings.capabilityGroup')}>
            <SettingsList>
              <SettingsRow
                label={t('presentation.settings.statusLabel')}
                description={
                  status === 'not_installed'
                    ? t('presentation.settings.notInstalledHint', {
                        size: formatBytes(capability.estimated_download_bytes),
                      })
                    : undefined
                }
                status={
                  <SettingsStatus tone={STATUS_TONE[capability.status]}>
                    {t(`presentation.status.${capability.status}`)}
                  </SettingsStatus>
                }
                controlLayout='actions'
                control={actions}
              />
              {installing && (
                <div className='px-16px pb-12px md:px-24px'>
                  <PresentationInstallProgress capability={capability} />
                </div>
              )}
              {capability.failure && (
                <SettingsRow
                  label={t('presentation.settings.lastFailure')}
                  description={
                    <span className='break-all text-danger-6' role='alert'>
                      {capability.failure.stage ? `${t(`presentation.stage.${capability.failure.stage}`)} · ` : ''}
                      {capability.failure.message}
                    </span>
                  }
                />
              )}
              {error && (
                <SettingsRow
                  label={t('presentation.settings.requestFailed')}
                  description={
                    <span className='break-all text-danger-6' role='alert'>
                      {error}
                    </span>
                  }
                />
              )}
            </SettingsList>
          </SettingsGroup>
        )}

        {capability && (
          <SettingsGroup title={t('presentation.settings.detailsGroup')}>
            <SettingsList>
              <SettingsRow label={t('presentation.settings.version')} description={`ppt-master ${capability.version} · ${capability.license}`} />
              <SettingsRow
                label={t('presentation.settings.commit')}
                description={<span className='font-mono text-12px'>{capability.source_commit.slice(0, 12)}</span>}
              />
              <SettingsRow
                label={t('presentation.settings.source')}
                description={<span className='break-all'>{capability.installed_from ?? capability.sources.join(' → ')}</span>}
              />
              {capability.installed_at_ms && (
                <SettingsRow label={t('presentation.settings.installedAt')} description={formatTime(capability.installed_at_ms)} />
              )}
              {capability.skill_dir && (
                <SettingsRow
                  label={t('presentation.settings.skillDir')}
                  description={<span className='break-all font-mono text-12px'>{capability.skill_dir}</span>}
                />
              )}
            </SettingsList>
          </SettingsGroup>
        )}
      </div>
    </SettingsPageWrapper>
  );
};

export default PresentationSettings;
