

import { ipcBridge } from '@/common';
import type { ComputerPermissionKind, ComputerPermissionStatus } from '@/common/adapter/ipcBridge';
import { configService } from '@/common/config/configService';
import {
  SettingsGroup,
  SettingsList,
  SettingsPageHeader,
  SettingsPermissionRow,
  SettingsRow,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import { Button, Switch } from '@arco-design/web-react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

const ComputerUseSettingsContent: React.FC = () => {
  const { t } = useTranslation();
  const [computerUse, setComputerUse] = useState(true);
  const [perm, setPerm] = useState<ComputerPermissionStatus | null>(null);

  useEffect(() => {
    setComputerUse(configService.get('agent.computerUse') ?? true);
  }, []);

  const refreshPerm = useCallback(() => {
    ipcBridge.computerPermissions.get
      .invoke()
      .then(setPerm)
      .catch(() => setPerm(null));
  }, []);

  useEffect(() => {
    refreshPerm();
    // Re-probe when the user returns from System Settings (the grant state is
    // the whole reason to revisit this panel).
    const onFocus = () => refreshPerm();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refreshPerm]);

  const handleComputerUseChange = useCallback((checked: boolean) => {
    setComputerUse(checked);
    configService.set('agent.computerUse', checked).catch(() => {
      setComputerUse(!checked);
      configService.setLocal('agent.computerUse', !checked);
    });
  }, []);

  // Register the app in the relevant TCC list (and show the OS prompt), then
  // jump to the exact System Settings pane so the user can flip the toggle.
  const grant = useCallback((kind: ComputerPermissionKind) => {
    ipcBridge.computerPermissions.request
      .invoke({ kind })
      .then(setPerm)
      .catch(() => {});
    ipcBridge.computerPermissions.openSettings.invoke({ kind }).catch(() => {});
  }, []);

  const isMac = perm?.platform === 'macos';
  const appLabel = perm?.app_label || 'Flowy';

  const permRow = (kind: ComputerPermissionKind, granted: boolean | null, label: string, description: string) => (
    <SettingsPermissionRow
      label={label}
      description={description}
      state={granted ? 'ready' : 'attention'}
      stateLabel={granted ? t('settings.computerUsePermGranted') : t('settings.computerUsePermNotInEffect')}
      controlLayout='actions'
      control={
        <Button size='small' type={granted ? 'secondary' : 'primary'} onClick={() => grant(kind)}>
          {t('settings.computerUseOpenSettings')}
        </Button>
      }
    />
  );

  return (
    <div className='w-full space-y-24px pb-16px'>
      <SettingsPageHeader
        title={t('settings.computerUseNav')}
        description={t('settings.computerUseDesc')}
      />
      <SettingsGroup title={t('settings.computerUseSection')}>
        <SettingsList>
          <SettingsRow
            label={t('settings.computerUse')}
            description={t('settings.computerUseDesc')}
            control={<Switch checked={computerUse} onChange={handleComputerUseChange} />}
            controlLayout='compact'
          />
        </SettingsList>
      </SettingsGroup>

      {isMac && (
        <SettingsGroup
          title={t('settings.computerUsePermSection')}
          action={<Button size='small' type='secondary' onClick={refreshPerm}>{t('settings.computerUsePermRefresh')}</Button>}
        >
          <SettingsList>
            {permRow('accessibility', perm?.accessibility ?? null, t('settings.computerUseAccessibility'), t('settings.computerUseAccessibilityDesc'))}
            {permRow('screen_recording', perm?.screen_recording ?? null, t('settings.computerUseScreenRecording'), t('settings.computerUseScreenRecordingDesc'))}
          </SettingsList>
          <div className='mt-8px px-4px text-12px leading-relaxed text-t-tertiary'>
            {t('settings.computerUseRestartHint', { app: appLabel })}
          </div>
        </SettingsGroup>
      )}
    </div>
  );
};

export default ComputerUseSettingsContent;
