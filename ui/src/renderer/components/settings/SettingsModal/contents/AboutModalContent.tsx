
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { httpGet } from '@/common/adapter/httpBridge';
import {
  DEVELOPER_MODE_REVEAL_TAP_COUNT,
  nextDeveloperModeRevealTap,
} from '@/common/config/developerMode';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
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
  const [taps, setTaps] = useState(0);
  const { uiEnabled } = useDeveloperModeGate();
  const [, setRevealed] = useConfig('system.developerModeUiRevealed');

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

  const handleAboutTitleClick = () => {
    if (uiEnabled) return;
    const next = nextDeveloperModeRevealTap(taps);
    setTaps(next.taps);
    if (!next.justRevealed) return;
    void setRevealed(true)
      .then(() => {
        Message.success(t('settings.developerMode.revealed'));
      })
      .catch(() => {
        setTaps(DEVELOPER_MODE_REVEAL_TAP_COUNT - 1);
        Message.error(t('settings.developerMode.enableFailed'));
      });
  };

  return (
    <div className='w-full space-y-24px pb-16px'>
      <SettingsPageHeader
        title='Flowy'
        description={t('settings.appDescription')}
        meta={<SettingsStatus>{`v${appVersion || '—'}`}</SettingsStatus>}
      />
      <SettingsList aria-label={t('settings.about')}>
        <SettingsRow
          label={
            <button
              type='button'
              className='m-0 appearance-none border-0 bg-transparent p-0 text-inherit font-inherit cursor-default select-none'
              onClick={handleAboutTitleClick}
            >
              {t('settings.about')}
            </button>
          }
          description={t('settings.appDescription')}
          control={<span className='text-13px tabular-nums text-t-secondary'>v{appVersion || '—'}</span>}
        />
      </SettingsList>
    </div>
  );
};

export default AboutModalContent;
