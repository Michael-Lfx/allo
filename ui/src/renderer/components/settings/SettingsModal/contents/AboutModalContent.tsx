/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Divider, Typography } from '@arco-design/web-react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import classNames from 'classnames';
import { useSettingsViewMode } from '../settingsViewContext';
import { httpGet } from '@/common/adapter/httpBridge';

// Real app version from the backend `/health` endpoint (public, no auth). The
// version there is `CARGO_PKG_VERSION`, which follows the single-source
// workspace version — so it stays correct in both the desktop shell and the
// WebUI browser without a Tauri-only `getVersion()` call.
const healthGet = httpGet<{ version?: string }>('/health');

const AboutModalContent: React.FC = () => {
  const { t } = useTranslation();
  const viewMode = useSettingsViewMode();
  const isPageMode = viewMode === 'page';

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
    <div className='flex flex-col h-full w-full'>
      <div
        className={classNames(
          'flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-24px',
          isPageMode && 'px-0 overflow-visible'
        )}
      >
        <div className='flex flex-col max-w-500px mx-auto'>
          <div className='flex flex-col items-center pb-24px'>
            <Typography.Title heading={3} className='text-24px font-bold text-t-primary mb-8px'>
              Flowy
            </Typography.Title>
            <Typography.Text className='text-14px text-t-secondary mb-12px text-center'>
              {t('settings.appDescription')}
            </Typography.Text>
            <div className='flex items-center justify-center gap-8px mb-16px'>
              <span className='px-10px py-4px rd-6px text-13px bg-fill-2 text-t-primary font-500'>
                v{appVersion || '—'}
              </span>
            </div>
          </div>

          <Divider className='my-16px' />
        </div>
      </div>
    </div>
  );
};

export default AboutModalContent;
