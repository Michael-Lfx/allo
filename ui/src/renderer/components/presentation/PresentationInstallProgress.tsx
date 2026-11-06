/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { IPresentationCapability } from '@/common/adapter/ipcBridge';
import {
  PRESENTATION_INSTALL_STAGES,
  formatBytes,
  stageIndex,
  stagePercent,
} from '@/renderer/hooks/presentation/usePresentationCapability';

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  );
  useEffect(() => {
    const query = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (!query) return undefined;
    const onChange = () => setReduced(query.matches);
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }, []);
  return reduced;
}

/**
 * One line of install status ("Step 2/5 · Downloading Python · 12 / 22 MB")
 * over a 2px hairline track driven only by `transform: scaleX` (ui-motion §3).
 */
const PresentationInstallProgress: React.FC<{ capability: IPresentationCapability }> = ({ capability }) => {
  const { t } = useTranslation();
  const reducedMotion = usePrefersReducedMotion();
  const stage = capability.stage;
  const percent = stagePercent(capability.progress);
  const progress = capability.progress;
  const bytes =
    progress && progress.total_bytes > 0
      ? `${formatBytes(progress.received_bytes)} / ${formatBytes(progress.total_bytes)}`
      : progress && progress.received_bytes > 0
        ? formatBytes(progress.received_bytes)
        : null;
  const ratio = percent === null ? 0 : percent / 100;

  return (
    <div className='flex min-w-0 flex-col gap-6px'>
      <div className='flex min-w-0 flex-wrap items-center gap-x-8px gap-y-2px text-12px leading-18px text-t-secondary'>
        <span className='tabular-nums text-t-tertiary'>
          {t('presentation.progress.step', {
            current: stageIndex(stage),
            total: PRESENTATION_INSTALL_STAGES.length,
          })}
        </span>
        <span className='text-t-primary'>{stage ? t(`presentation.stage.${stage}`) : t('presentation.status.installing')}</span>
        {bytes && <span className='tabular-nums'>{bytes}</span>}
        {percent !== null && <span className='tabular-nums'>{percent}%</span>}
      </div>
      <div aria-hidden='true' className='relative h-2px w-full overflow-hidden rounded-full bg-fill-2'>
        <div
          className='h-full bg-primary-6'
          style={{
            transform: `scaleX(${ratio})`,
            transformOrigin: 'left center',
            transition: reducedMotion ? 'none' : 'transform 180ms cubic-bezier(0.16, 1, 0.3, 1)',
          }}
        />
      </div>
    </div>
  );
};

export default PresentationInstallProgress;
