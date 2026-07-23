/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  getTurnStatusLabel,
  type TurnPresentationState,
} from '@/renderer/pages/conversation/platforms/turnPresentationState';

type TurnStatusRailProps = {
  presentation: TurnPresentationState;
  completionSummary?: {
    elapsedMs?: number;
    toolCount?: number;
    changedFileCount?: number;
  };
};

const formatElapsed = (ms: number): string => {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
};

const TurnStatusRail: React.FC<TurnStatusRailProps> = ({ presentation, completionSummary }) => {
  const { t } = useTranslation();
  const [showReceipt, setShowReceipt] = useState(false);

  useEffect(() => {
    if (presentation.phase !== 'completed') {
      setShowReceipt(false);
      return;
    }
    setShowReceipt(true);
    const timer = window.setTimeout(() => setShowReceipt(false), 4200);
    return () => window.clearTimeout(timer);
  }, [presentation.phase, presentation.finishedAt]);

  const label = useMemo(
    () => getTurnStatusLabel(presentation.phase, presentation.detail, t),
    [presentation.detail, presentation.phase, t]
  );

  if (presentation.showStatusRail && label) {
    return (
      <div
        className='turn-status-rail mx-auto mb-8px max-w-780px px-8px text-12px text-t-secondary flex items-center gap-8px min-h-20px'
        role='status'
        aria-live='polite'
        data-testid='turn-status-rail'
        data-phase={presentation.phase}
      >
        <span
          className={`inline-block w-6px h-6px rd-full shrink-0 ${
            presentation.phase === 'waiting_permission' ? 'bg-warning' : 'bg-primary animate-pulse'
          }`}
          aria-hidden='true'
        />
        <span className='truncate'>{label}</span>
      </div>
    );
  }

  if (showReceipt && presentation.phase === 'completed') {
    const elapsed =
      typeof completionSummary?.elapsedMs === 'number'
        ? completionSummary.elapsedMs
        : presentation.startedAt != null && presentation.finishedAt != null
          ? presentation.finishedAt - presentation.startedAt
          : undefined;
    const parts: string[] = [];
    if (elapsed != null && elapsed >= 0) {
      parts.push(
        t('conversation.turnStatus.completedIn', {
          defaultValue: 'Done in {{time}}',
          time: formatElapsed(elapsed),
        })
      );
    }
    if (completionSummary?.changedFileCount && completionSummary.changedFileCount > 0) {
      parts.push(
        t('conversation.turnStatus.changedFiles', {
          defaultValue: 'Changed {{count}} files',
          count: completionSummary.changedFileCount,
        })
      );
    } else if (completionSummary?.toolCount && completionSummary.toolCount > 0) {
      parts.push(
        t('conversation.turnStatus.toolsUsed', {
          defaultValue: '{{count}} tools',
          count: completionSummary.toolCount,
        })
      );
    }
    if (parts.length === 0) return null;
    return (
      <div
        className='turn-completion-receipt mx-auto mb-8px max-w-780px px-8px text-12px text-t-secondary min-h-20px'
        role='status'
        aria-live='polite'
        data-testid='turn-completion-receipt'
      >
        {parts.join(' · ')}
      </div>
    );
  }

  return <div className='min-h-0' aria-hidden='true' />;
};

export default TurnStatusRail;
