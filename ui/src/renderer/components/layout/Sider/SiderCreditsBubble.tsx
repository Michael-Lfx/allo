/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Attention, CloseSmall } from '@icon-park/react';
import classNames from 'classnames';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
import { openOfficialWebsiteCredits } from '@/renderer/utils/openOfficialWebsiteCredits';
import {
  resolveCreditsBubbleState,
  isCreditsBubbleDismissed,
  dismissCreditsBubble,
  type CreditsBubbleState,
} from '@/renderer/utils/credits/creditsBubbleModel';

export interface SiderCreditsBubbleProps {
  collapsed: boolean;
  isMobile?: boolean;
}

const SiderCreditsBubble: React.FC<SiderCreditsBubbleProps> = ({ collapsed, isMobile = false }) => {
  const { t } = useTranslation();
  const { balance, authenticated, isFetchingBalance, lastRefreshAt } = useCredits();
  const { whoami } = useCloudAuth();
  const [dismissedState, setDismissedState] = useState<CreditsBubbleState | null>(null);

  const bubbleState = resolveCreditsBubbleState({
    balance,
    authenticated,
    isFetchingBalance,
    lastRefreshAt,
  });

  const userId = whoami?.id;

  // Check if current state has been dismissed for today
  const isDismissed =
    bubbleState === 'normal' ||
    dismissedState === bubbleState ||
    isCreditsBubbleDismissed(userId, 'sider', bubbleState);

  // If bubble state shifts (e.g. from low to exhausted), un-dismiss to alert the user
  useEffect(() => {
    if (bubbleState === 'exhausted' && dismissedState === 'low') {
      setDismissedState(null);
    }
  }, [bubbleState, dismissedState]);

  if (bubbleState === 'normal' || isDismissed) {
    return null;
  }

  const isExhausted = bubbleState === 'exhausted';

  const handleDismiss = (event: React.MouseEvent) => {
    event.stopPropagation();
    dismissCreditsBubble(userId, 'sider', bubbleState);
    setDismissedState(bubbleState);
  };

  const handleTopUp = (event: React.MouseEvent) => {
    event.stopPropagation();
    void openOfficialWebsiteCredits(undefined, undefined, {
      source: 'sider',
      balance,
    });
  };

  const title = isExhausted
    ? t('common.creditsBubble.exhaustedTitle', { defaultValue: '积分已耗尽' })
    : t('common.creditsBubble.lowTitle', { defaultValue: '积分即将耗尽' });

  const desc = isExhausted
    ? t('common.creditsBubble.exhaustedDesc', {
        defaultValue: '当前积分余额为 0，无法继续发起对话，请先充值',
      })
    : t('common.creditsBubble.lowDesc', {
        defaultValue: '当前积分低于 1,000，建议及时补充以保障对话连续性',
      });

  const actionText = isExhausted
    ? t('common.creditsBubble.exhaustedAction', { defaultValue: '立即充值' })
    : t('common.creditsBubble.lowAction', { defaultValue: '去充值' });

  return (
    <div
      role='alert'
      aria-live='polite'
      data-testid='sider-credits-bubble'
      className={classNames(
        'credits-bubble-card p-12px flex flex-col gap-8px',
        isExhausted ? 'credits-bubble-card--exhausted' : 'credits-bubble-card--low',
        collapsed ? 'sider-credits-bubble--collapsed credits-bubble-enter-side' : 'sider-credits-bubble--expanded credits-bubble-enter',
        isMobile && 'sider-credits-bubble--mobile'
      )}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div className='flex items-center justify-between gap-6px'>
        <div className='flex items-center gap-6px min-w-0'>
          <span
            className={classNames(
              'shrink-0 flex items-center justify-center size-15px rd-full',
              isExhausted ? 'text-[var(--danger)]' : 'text-[var(--flowy-attention)]'
            )}
          >
            <Attention theme='filled' size='14' fill='currentColor' />
          </span>
          <span className='text-12px font-600 leading-16px text-t-primary truncate'>{title}</span>
        </div>
        <button
          type='button'
          aria-label={t('common.creditsBubble.close', { defaultValue: '关闭提示' })}
          className='shrink-0 flex items-center justify-center size-16px rd-4px text-t-tertiary hover:text-t-primary hover:bg-fill-2 transition-colors border-none bg-transparent cursor-pointer p-0'
          onClick={handleDismiss}
        >
          <CloseSmall theme='outline' size='14' fill='currentColor' />
        </button>
      </div>

      <div className='text-12px leading-16px text-t-secondary'>{desc}</div>

      <div className='flex items-center justify-end pt-4px border-t border-[var(--border-subtle,rgba(255,255,255,0.06))]'>
        <button
          type='button'
          className={classNames(
            'px-12px py-3px rd-5px text-11px font-600 border-none cursor-pointer transition-opacity hover:opacity-90 active:opacity-80',
            isExhausted
              ? 'bg-[var(--danger)] text-white'
              : 'bg-[var(--primary-6)] text-white'
          )}
          onClick={handleTopUp}
        >
          {actionText}
        </button>
      </div>

      {collapsed ? (
        <div className='sider-credits-bubble-arrow-left' aria-hidden='true' />
      ) : (
        <div className='sider-credits-bubble-arrow-down' aria-hidden='true' />
      )}
    </div>
  );
};

export default SiderCreditsBubble;
