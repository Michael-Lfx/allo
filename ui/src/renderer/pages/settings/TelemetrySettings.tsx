import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Switch } from '@arco-design/web-react';
import {
  SettingsGroup,
  SettingsList,
  SettingsPageHeader,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';
import { getInstallId } from '@/renderer/utils/analytics/identity';
import {
  isTelemetryConfigured,
  isTelemetryOptedOut,
  setTelemetryOptOut,
} from '@/renderer/utils/analytics/telemetry';

const TelemetrySettings: React.FC = () => {
  const { t } = useTranslation();
  const [optedOut, setOptedOut] = useState(() => isTelemetryOptedOut());
  const configured = isTelemetryConfigured();
  const installId = useMemo(() => getInstallId(), []);

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader title={t('telemetry.title')} description={t('telemetry.description')} />
        <SettingsGroup title={t('telemetry.title')}>
          <SettingsList>
            <PreferenceRow
              label={t('telemetry.enabled')}
              description={t('telemetry.enabledHint')}
              controlLayout='compact'
            >
              <Switch
                checked={!optedOut}
                onChange={(checked) => {
                  setOptedOut(!checked);
                  setTelemetryOptOut(!checked);
                }}
              />
            </PreferenceRow>
            <PreferenceRow
              label={t('telemetry.installId')}
              description={t('telemetry.installIdHint')}
              controlLayout='compact'
            >
              <span className='max-w-300px break-all text-right text-12px text-t-secondary'>{installId}</span>
            </PreferenceRow>
            <PreferenceRow
              label={t('telemetry.sdkStatus')}
              controlLayout='compact'
            >
              <span className='max-w-300px text-right text-12px text-t-secondary'>
                {configured ? t('telemetry.configured') : t('telemetry.notConfigured')}
              </span>
            </PreferenceRow>
          </SettingsList>
        </SettingsGroup>
      </div>
    </SettingsPageWrapper>
  );
};

export default TelemetrySettings;
