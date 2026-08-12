

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { httpGet } from '@/common/adapter/httpBridge';
import {
  SettingsList,
  SettingsPageHeader,
  SettingsRow,
  SettingsStatus,
} from '@/renderer/components/settings/SettingsPagePrimitives';

// Real app version from the backend `/health` endpoint (public, no auth). The
// version there is `CARGO_PKG_VERSION`, which follows the single-source
// workspace version — so it stays correct in both the desktop shell and the
// WebUI browser without a Tauri-only `getVersion()` call.
const healthGet = httpGet<{ version?: string }>('/health');

const AboutModalContent: React.FC = () => {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState('');

  useEffect(() => {
    let alive = true;
    healthGet
      .invoke()
      .then((health) => {
        if (alive && health?.version) setAppVersion(health.version);
      })
      .catch((error) => console.error('Failed to read app version:', error));
    return () => {
      alive = false;
    };
  }, []);

  return (
    <div className='w-full space-y-24px pb-16px'>
      <SettingsPageHeader
        title='Flowy'
        description={t('settings.appDescription')}
        meta={<SettingsStatus>{`v${appVersion || '—'}`}</SettingsStatus>}
      />
      <SettingsList aria-label={t('settings.about')}>
        <SettingsRow
          label={t('settings.about')}
          description={t('settings.appDescription')}
          control={<span className='text-13px tabular-nums text-t-secondary'>v{appVersion || '—'}</span>}
        />
      </SettingsList>
    </div>
  );
};

export default AboutModalContent;
