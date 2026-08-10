/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect } from 'react';
import type { WorkspaceTabProps } from '../../types';
import SuggestionsTab from '../../../tabs/SuggestionsTab';
import LearningGraphTab from '../../../tabs/LearningGraphTab';
import AnalyticsTab from '../../../tabs/AnalyticsTab';

/**
 * 洞察 — what the companion has worked out and what it wants to tell you:
 * the proactive suggestion cards, the memory/skill graph it grew, and the
 * local usage roll-up. Read-only apart from accepting or dismissing a card.
 */
const InsightsTab: React.FC<WorkspaceTabProps> = ({ companionId, onAttentionChange }) => {
  useEffect(() => {
    onAttentionChange?.(false);
  }, [onAttentionChange]);

  return (
    <div className='flex flex-col gap-16px py-8px'>
      <SuggestionsTab />
      <LearningGraphTab companionId={companionId} />
      <AnalyticsTab />
    </div>
  );
};

export default InsightsTab;
