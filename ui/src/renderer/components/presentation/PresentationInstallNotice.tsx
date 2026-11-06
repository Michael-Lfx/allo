/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Button } from '@arco-design/web-react';
import type { PresentationCapabilityStatus } from '@/common/adapter/ipcBridge';
import { usePresentationCapability } from '@/renderer/hooks/presentation/usePresentationCapability';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import PresentationInstallProgress from './PresentationInstallProgress';

/** "Installed" is one-shot feedback after an install this surface watched. */
const SETTLED_AUTO_DISMISS_MS = 6_000;

/**
 * Conversation rail line for the instance-wide ppt-master install. Nomi
 * starts the install itself (`nomi_presentation_install`), so this only
 * appears while an install runs, or briefly after one this view watched has
 * settled. Same visual language as GoalStatusNotice (dot + 12px text).
 */
const PresentationInstallNotice: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { capability, pending, cancel, install } = usePresentationCapability();
  // The settings page is developer-only, so only link to it when it is visible.
  const { active: developerMode } = useDeveloperModeGate();
  const [settled, setSettled] = useState<PresentationCapabilityStatus | null>(null);
  const previous = useRef<PresentationCapabilityStatus | undefined>(undefined);

  const status = capability?.status;
  useEffect(() => {
    const before = previous.current;
    previous.current = status;
    if (before === 'installing' && status && status !== 'installing') {
      setSettled(status);
    } else if (status === 'installing') {
      setSettled(null);
    }
  }, [status]);

  useEffect(() => {
    if (settled !== 'ready' && settled !== 'not_installed') return undefined;
    const timer = window.setTimeout(() => setSettled(null), SETTLED_AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [settled]);

  if (!capability) return null;
  const installing = status === 'installing';
  if (!installing && !settled) return null;

  const tone = installing
    ? 'bg-primary animate-pulse'
    : settled === 'ready'
      ? 'bg-success'
      : settled === 'failed'
        ? 'bg-danger'
        : 'bg-6';
  const label = installing
    ? t('presentation.notice.installing')
    : settled === 'ready'
      ? t('presentation.notice.ready')
      : settled === 'failed'
        ? t('presentation.notice.failed')
        : t('presentation.notice.cancelled');

  return (
    <div
      className='presentation-install-notice mx-auto mb-4px flex w-full max-w-780px flex-col gap-4px px-8px text-12px text-t-secondary'
      role='status'
      aria-live='polite'
      data-testid='presentation-install-notice'
      data-presentation-status={status}
    >
      <div className='flex min-h-20px items-center gap-8px'>
        <span className={`inline-block h-6px w-6px shrink-0 rd-full ${tone}`} aria-hidden='true' />
        <span className='truncate'>{label}</span>
        <span className='ml-auto flex shrink-0 items-center gap-2px'>
          {installing && (
            <Button size='mini' type='text' loading={pending === 'cancel'} onClick={() => void cancel()}>
              {t('presentation.actions.cancel')}
            </Button>
          )}
          {settled === 'failed' && (
            <>
              <Button size='mini' type='text' loading={pending === 'install'} onClick={() => void install()}>
                {t('presentation.actions.retry')}
              </Button>
              {developerMode && (
                <Button size='mini' type='text' onClick={() => navigate('/settings/presentation')}>
                  {t('presentation.notice.details')}
                </Button>
              )}
            </>
          )}
        </span>
      </div>
      {installing && <PresentationInstallProgress capability={capability} />}
      {settled === 'failed' && capability.failure && (
        <div className='truncate text-t-tertiary' title={capability.failure.message}>
          {capability.failure.message}
        </div>
      )}
    </div>
  );
};

export default PresentationInstallNotice;
