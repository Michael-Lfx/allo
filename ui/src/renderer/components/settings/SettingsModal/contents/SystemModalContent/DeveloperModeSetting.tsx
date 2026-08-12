/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { configService } from '@/common/config/configService';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { SettingsRow } from '@/renderer/components/settings/SettingsPagePrimitives';
import { Message, Switch } from '@arco-design/web-react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import DeveloperModePasswordModal from './DeveloperModePasswordModal';

const DeveloperModeSetting: React.FC = () => {
  const { t } = useTranslation();
  const [developerMode, setDeveloperMode] = useConfig('system.developerMode');
  const [passwordModalVisible, setPasswordModalVisible] = useState(false);
  const [saving, setSaving] = useState(false);

  const enabled = developerMode === true;

  const handleToggle = (checked: boolean) => {
    if (checked) {
      setPasswordModalVisible(true);
      return;
    }

    setDeveloperMode(false).catch(() => {
      configService.setLocal('system.developerMode', true);
      Message.error(t('settings.developerMode.disableFailed'));
    });
  };

  const handleEnableConfirm = async () => {
    setSaving(true);
    try {
      await setDeveloperMode(true);
      setPasswordModalVisible(false);
      Message.success(t('settings.developerMode.enabled'));
    } catch {
      Message.error(t('settings.developerMode.enableFailed'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <SettingsRow
        label={t('settings.developerMode.title')}
        description={t('settings.developerMode.description')}
        control={<Switch checked={enabled} onChange={handleToggle} />}
        controlLayout='compact'
      />

      <DeveloperModePasswordModal
        visible={passwordModalVisible}
        loading={saving}
        onClose={() => setPasswordModalVisible(false)}
        onConfirm={handleEnableConfirm}
      />
    </>
  );
};

export default DeveloperModeSetting;
