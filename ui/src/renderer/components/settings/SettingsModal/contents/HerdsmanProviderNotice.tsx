/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { isHerdsmanProvider } from '@/common/types/provider/herdsman';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import { isWindows } from '@/renderer/utils/platform';
import React from 'react';
import { useTranslation } from 'react-i18next';

type HerdsmanPresence = 'searching' | 'connected' | 'disconnected';

const presenceTone: Record<HerdsmanPresence, string> = {
  searching: 'bg-[var(--color-text-3)]',
  connected: 'bg-[rgb(var(--success-6))]',
  disconnected: 'bg-[rgb(var(--warning-6))]',
};

const presenceCopy = {
  searching: {
    title: 'settings.modelHub.herdsman.searchingTitle',
    body: 'settings.modelHub.herdsman.searchingBody',
  },
  connected: {
    title: 'settings.modelHub.herdsman.connectedTitle',
    body: 'settings.modelHub.herdsman.connectedBody',
  },
  disconnected: {
    title: 'settings.modelHub.herdsman.disconnectedTitle',
    body: 'settings.modelHub.herdsman.disconnectedBody',
  },
} as const;

/**
 * Read-only status for the engine-owned provider. The endpoint and API key
 * stay on the backend; this card only tells the user whether running local
 * models can currently be selected in a conversation.
 */
export const HerdsmanProviderNotice: React.FC = () => {
  const { t } = useTranslation();
  const { data } = useProvidersQuery();
  const herdsman = (data ?? []).find((provider) => isHerdsmanProvider(provider));
  if (!isWindows() && !herdsman) return null;

  const presence: HerdsmanPresence = !herdsman
    ? 'searching'
    : herdsman.enabled === false
      ? 'disconnected'
      : 'connected';

  return (
    <div className='rd-10px border border-solid border-[var(--color-border-2)] bg-1 px-12px py-10px flex items-start gap-10px'>
      <span
        className={`mt-6px h-8px w-8px shrink-0 rd-full ${presenceTone[presence]}`}
        aria-hidden
      />
      <div className='min-w-0'>
        <div className='text-13px font-600 leading-18px text-t-primary'>
          {t(presenceCopy[presence].title)}
        </div>
        <div className='mt-2px text-12px leading-18px text-t-secondary'>
          {t(presenceCopy[presence].body)}
        </div>
      </div>
    </div>
  );
};
