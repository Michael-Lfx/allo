

import React, { useMemo } from 'react';
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

const shouldShowLiveRail = (phase: TurnPresentationState['phase']): boolean => {
  switch (phase) {
    case 'waiting_permission':
      return true;
    case 'idle':
    case 'local_pending':
    case 'accepted':
    case 'preparing':
    case 'thinking':
    case 'streaming':
    case 'tooling':
    case 'finalizing':
    case 'completed':
    case 'failed':
    case 'cancelled':
      return false;
    default: {
      const exhaustive: never = phase;
      return exhaustive;
    }
  }
};

const TurnStatusRail: React.FC<TurnStatusRailProps> = ({ presentation }) => {
  const { t } = useTranslation();

  const label = useMemo(
    () => getTurnStatusLabel(presentation.phase, presentation.detail, t),
    [presentation.detail, presentation.phase, t]
  );

  if (presentation.showStatusRail && shouldShowLiveRail(presentation.phase) && label) {
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

  return null;
};

export default TurnStatusRail;
