/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Empty } from '@arco-design/web-react';
import React, { useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useConversationPlan } from './conversationPlanContext';
import PlanTodoList, { PlanThinkingOrb } from './PlanTodoList';
import styles from './planTodoList.module.css';

const ConversationPlanPanel: React.FC = () => {
  const { t } = useTranslation();
  const { plan } = useConversationPlan();
  const inProgressRowRef = useRef<HTMLDivElement | null>(null);
  const working = Boolean(plan && plan.done < plan.total);
  const spineFill = useMemo(() => {
    if (!plan || plan.total <= 0) return '0%';
    const liveOffset = plan.entries.some((entry) => entry.status === 'in_progress') ? 0.5 : 0;
    return `${Math.min(100, ((plan.done + liveOffset) / plan.total) * 100)}%`;
  }, [plan]);

  useEffect(() => {
    const row = inProgressRowRef.current;
    if (!row) return;
    const reduce =
      typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    row.scrollIntoView({ block: 'nearest', behavior: reduce ? 'auto' : 'smooth' });
  }, [plan?.done, plan?.entries, plan?.total]);

  if (!plan) {
    return (
      <div className={styles.empty} data-testid='conversation-plan-panel'>
        <PlanThinkingOrb state='settled' />
        <Empty
          description={
            <div className='text-center'>
              <div className='text-14px font-600 text-t-secondary'>
                {t('conversation.workspace.plan.empty', { defaultValue: 'No plan yet' })}
              </div>
              <div className='mt-4px text-12px leading-18px text-t-tertiary'>
                {t('conversation.workspace.plan.emptyDescription', {
                  defaultValue: 'When the agent starts a plan, its tasks will show up here',
                })}
              </div>
            </div>
          }
        />
      </div>
    );
  }

  const statusLabel =
    plan.done >= plan.total
      ? t('conversation.workspace.plan.completed', { defaultValue: 'Completed' })
      : t('conversation.workspace.plan.inProgress', { defaultValue: 'In progress' });

  return (
    <div className={styles.stage} data-testid='conversation-plan-panel'>
      <div className={styles.track} style={{ '--plan-spine-fill': spineFill } as React.CSSProperties}>
        <span className={styles.spine} data-testid='conversation-plan-spine' aria-hidden='true'>
          <span className={styles.spineFill} />
        </span>
        <div data-testid='conversation-plan-header' className={`${styles.header} ${working ? styles.headerLive : styles.headerSettled}`}>
          <span className={styles.glyph}>
            <PlanThinkingOrb state={working ? 'working' : 'settled'} />
          </span>
          <span className={styles.status}>{statusLabel}</span>
          <span className={styles.countPill}>
            {t('messages.planProgress', {
              done: plan.done,
              total: plan.total,
              defaultValue: '{{done}}/{{total}}',
            })}
          </span>
        </div>
        <PlanTodoList
          entries={plan.entries}
          variant='panel'
          listTestId='conversation-plan-list'
          inProgressRowRef={inProgressRowRef}
        />
      </div>
    </div>
  );
};

export default ConversationPlanPanel;
