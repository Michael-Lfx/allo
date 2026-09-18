/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useMessageList } from '@renderer/pages/conversation/Messages/hooks';
import { useConversationPlan } from './conversationPlanContext';
import PlanTodoList from './PlanTodoList';
import { derivePinnedPlan, type PinnedPlanData } from './pinnedPlanModel';
import styles from './planTodoList.module.css';

/**
 * Pinned plan bar: sits in document flow above the composer so it never covers
 * the last messages. Desktop (with a workspace) opens the right-rail plan tab;
 * mobile and workspace-less surfaces expand the compact checklist in place.
 */
const PinnedPlan: React.FC<{ plan?: PinnedPlanData | null; active?: boolean; className?: string }> = ({
  plan: suppliedPlan,
  active: suppliedActive,
  className = 'w-fit max-w-[calc(100vw-32px)]',
}) => {
  const { t } = useTranslation();
  const { canOpenPlanTab, openPlanTab } = useConversationPlan();
  const list = useMessageList();
  const derivedPlan = useMemo(
    () => (suppliedPlan === undefined ? derivePinnedPlan(list) : null),
    [list, suppliedPlan]
  );
  const plan = suppliedPlan === undefined ? derivedPlan : suppliedPlan;
  const [expanded, setExpanded] = useState(false);

  if (!plan) return null;

  const { entries, done, total } = plan;
  const active = suppliedActive ?? plan.active;
  const showInPlaceList = !canOpenPlanTab && expanded;

  const handleSummaryActivate = () => {
    if (canOpenPlanTab) {
      openPlanTab();
      return;
    }
    setExpanded((value) => !value);
  };

  const handleSummaryKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    handleSummaryActivate();
  };

  return (
    <div data-testid='pinned-plan-bar' className={`flex flex-col items-center ${className}`}>
      {showInPlaceList && (
        <div
          data-testid='pinned-plan-popover'
          className='w-[min(320px,calc(100vw-32px))] mb-6px'
        >
          <PlanTodoList entries={entries} variant='compact' listTestId='pinned-plan-list' />
        </div>
      )}
      <div
        role='button'
        tabIndex={0}
        aria-expanded={canOpenPlanTab ? undefined : expanded}
        data-testid='pinned-plan-summary'
        className={styles.chip}
        onClick={handleSummaryActivate}
        onKeyDown={handleSummaryKeyDown}
      >
        {active && done < total && (
          <span
            aria-hidden='true'
            data-testid='pinned-plan-progress-indicator'
            className={styles.chipPulse}
          />
        )}
        <span className={styles.chipLabel}>
          {t('messages.planTodoList', { defaultValue: 'Task queue' })}
        </span>
        <span aria-hidden='true' className={styles.chipDivider} />
        <span className={styles.chipMeta}>
          {t('messages.planProgress', { done, total, defaultValue: '{{done}}/{{total}}' })}
        </span>
      </div>
    </div>
  );
};

export default PinnedPlan;
