/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Typography } from '@arco-design/web-react';
import OpenCapabilitiesPage from '@renderer/pages/openCapabilities';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const OpenCapabilitiesSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <SettingsPageWrapper contentClassName='md:max-w-1180px'>
      <div className='flex flex-col gap-20px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('settings.openCapabilities.title', { defaultValue: '远程&开放能力' })}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('settings.openCapabilities.subtitle', {
              defaultValue: '分开管理 WebUI 访问入口，以及 Flowy Remote MCP / REST 对外开放能力。',
            })}
          </Typography.Paragraph>
        </div>
        <OpenCapabilitiesPage variant='settings' />
      </div>
    </SettingsPageWrapper>
  );
};

export default OpenCapabilitiesSettings;
