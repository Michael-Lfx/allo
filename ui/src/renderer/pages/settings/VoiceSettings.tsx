/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';
import {
  SettingsList,
  SettingsPageHeader,
  SettingsRow,
  SettingsSection,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import SettingsPageWrapper from './components/SettingsPageWrapper';

export type VoiceModelInstallStatus =
  | 'NotInstalled'
  | 'ProbingSource'
  | 'Downloading'
  | 'VerifyingSha256'
  | 'Ready'
  | 'Failed'
  | 'Removing';

export interface VoiceModelStatusPayload {
  state: {
    status: VoiceModelInstallStatus;
    detail?: Record<string, unknown>;
  };
  models_dir?: string;
  total_size_bytes?: number;
  files?: Array<{
    path: string;
    size_bytes: number;
    exists: boolean;
  }>;
}

const VoiceSettings: React.FC = () => {
  const { t } = useTranslation();
  const [modelStatus, setModelStatus] = useState<VoiceModelInstallStatus>('NotInstalled');
  const [loading, setLoading] = useState(true);

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models/status`, {
        headers: buildBackendAuthHeaders('GET'),
      });
      if (response.ok) {
        const data = (await response.json()) as VoiceModelStatusPayload;
        if (data?.state?.status) {
          setModelStatus(data.state.status);
        }
      }
    } catch {
      // Backend status probe is best-effort in skeleton stage; default to NotInstalled
      setModelStatus('NotInstalled');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchStatus();
  }, [fetchStatus]);

  // Motion token bus: progress track driven by transform: scaleX() only.
  // Idle state is opacity: 0 with scaleX(0); no layout dimension animations.
  const progressRatio = 0;
  const isProgressVisible = modelStatus === 'Downloading' || modelStatus === 'VerifyingSha256';

  return (
    <SettingsPageWrapper layout='form'>
      <div className='flex flex-col gap-24px'>
        <SettingsPageHeader
          title={t('voice.title')}
          description={t('voice.settings.sectionDesc')}
        />

        <SettingsSection
          title={t('voice.settings.sectionTitle')}
          description={t('voice.settings.sectionDesc')}
        >
          <SettingsList>
            <div className='relative overflow-hidden'>
              <SettingsRow
                label={t('voice.phases.notInstalled')}
                description={t('voice.phases.notInstalledDesc')}
                controlLayout='actions'
                control={
                  <div className='w-240px shrink-0 flex items-center justify-end'>
                    <Button
                      type='primary'
                      disabled={loading || modelStatus === 'NotInstalled'}
                      className='tabular-nums'
                    >
                      {t('voice.actions.downloadAndEnable')}
                    </Button>
                  </div>
                }
              />
              {/*
                ui-motion.zh.md §3 contract:
                2px hairline track permanently rendered at the bottom of the row.
                Driven solely by transform: scaleX() (never animates width/height).
              */}
              <div
                aria-hidden='true'
                className='pointer-events-none absolute bottom-0 left-0 right-0 h-2px bg-fill-2 transition-opacity duration-180 ease-out'
                style={{ opacity: isProgressVisible ? 1 : 0 }}
              >
                <div
                  className='h-full bg-primary-6 transition-transform duration-180 ease-out'
                  style={{
                    transform: `scaleX(${progressRatio})`,
                    transformOrigin: 'left center',
                  }}
                />
              </div>
            </div>
          </SettingsList>
        </SettingsSection>
      </div>
    </SettingsPageWrapper>
  );
};

export default VoiceSettings;
