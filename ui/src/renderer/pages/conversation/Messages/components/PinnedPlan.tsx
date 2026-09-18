
import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThinkingOrb } from 'thinking-orbs';
import { useMessageList } from '@renderer/pages/conversation/Messages/hooks';
import { useConversationPlan } from './conversationPlanContext';
import PlanTodoList from './PlanTodoList';
import { derivePinnedPlan, type PinnedPlanData } from './pinnedPlanModel';

/**
 * Pinned plan bar: centered above the composer, it surfaces the conversation's
 * current plan (the latest `plan` message) without competing with the command
 * queue. Desktop (with a workspace) opens the right-rail plan tab; mobile and
 * workspace-less surfaces expand the compact checklist in place. Renders
 * nothing when there is no active plan.
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
    <div data-testid='pinned-plan-bar' className={`relative ${className}`}>
      <div
        role='button'
        tabIndex={0}
        aria-expanded={canOpenPlanTab ? undefined : expanded}
        data-testid='pinned-plan-summary'
        className='flex h-28px items-center gap-6px rd-full px-10px cursor-pointer select-none'
        style={{
          background: 'var(--color-bg-1)',
          border: '1px solid color-mix(in srgb, rgb(var(--primary-6)) 14%, var(--color-border-2))',
          boxShadow: 'none',
          color: 'var(--text-secondary)',
        }}
        onClick={handleSummaryActivate}
        onKeyDown={handleSummaryKeyDown}
      >
        {active && done < total && (
          <ThinkingOrb
            aria-hidden='true'
            data-testid='pinned-plan-progress-indicator'
            state='working'
            size={20}
            theme='auto'
            className='block shrink-0'
          />
        )}
        <span className='min-w-0 truncate text-12px font-600 leading-none'>
          {t('messages.planTodoList', { defaultValue: 'Task queue' })}
        </span>
        <span aria-hidden='true' className='h-12px w-1px shrink-0 bg-[var(--color-border-2)]' />
        <span className='whitespace-nowrap text-12px leading-none tabular-nums'>
          {t('messages.planProgress', { done, total, defaultValue: '{{done}}/{{total}}' })}
        </span>
      </div>

      {showInPlaceList && (
        <div
          data-testid='pinned-plan-popover'
          className='absolute left-1/2 w-[min(320px,calc(100vw-32px))] -translate-x-1/2 bottom-full z-10 pb-4px'
        >
          <PlanTodoList entries={entries} variant='compact' listTestId='pinned-plan-list' />
        </div>
      )}
    </div>
  );
};

export default PinnedPlan;
