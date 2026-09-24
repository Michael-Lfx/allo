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

export interface SendBoxCreditsBubbleProps {
  isProcessing?: boolean;
  isFocused?: boolean;
  hasDraft?: boolean;
  blockedTriggerCount?: number;
}

const SendBoxCreditsBubble: React.FC<SendBoxCreditsBubbleProps> = ({
  isProcessing = false,
  isFocused = false,
  hasDraft = false,
  blockedTriggerCount = 0,
}) => {
  const { t } = useTranslation();
  const { balance, authenticated, isFetchingBalance, lastRefreshAt } = useCredits();
  const { whoami } = useCloudAuth();
  const [dismissedState, setDismissedState] = useState<CreditsBubbleState | null>(null);
  const [forceVisibleCount, setForceVisibleCount] = useState(blockedTriggerCount);

  const bubbleState = resolveCreditsBubbleState({
    balance,
    authenticated,
    isFetchingBalance,
    lastRefreshAt,
  });

  const userId = whoami?.id;

  // If user clicked send while exhausted, un-dismiss and force bubble visibility
  useEffect(() => {
    if (blockedTriggerCount > forceVisibleCount) {
      setForceVisibleCount(blockedTriggerCount);
      setDismissedState(null);
    }
  }, [blockedTriggerCount, forceVisibleCount]);

  if (bubbleState === 'normal' || isProcessing) {
    return null;
  }

  const isExhausted = bubbleState === 'exhausted';
  const isForced = isExhausted && blockedTriggerCount > 0;

  // In low state, only show when focused or has draft
  if (!isExhausted && !isFocused && !hasDraft) {
    return null;
  }

  // Check dismiss state (unless force-triggered by an attempt to send)
  if (!isForced) {
    const isDismissed =
      dismissedState === bubbleState ||
      isCreditsBubbleDismissed(userId, 'sendbox', bubbleState);
    if (isDismissed) {
      return null;
    }
  }

  const handleDismiss = (event: React.MouseEvent) => {
    event.stopPropagation();
    dismissCreditsBubble(userId, 'sendbox', bubbleState);
    setDismissedState(bubbleState);
  };

  const handleTopUp = (event: React.MouseEvent) => {
    event.stopPropagation();
    void openOfficialWebsiteCredits(undefined, undefined, {
      source: 'sendbox',
      balance,
    });
  };

  const text = isExhausted
    ? t('conversation.sendBox.creditsExhaustedBlocked', {
        defaultValue: '积分已耗尽，请充值后发送',
      })
    : t('conversation.sendBox.creditsLowHint', {
        defaultValue: '积分不足 1,000，建议及时充值',
      });

  const actionText = isExhausted
    ? t('common.creditsBubble.exhaustedAction', { defaultValue: '立即充值' })
    : t('common.creditsBubble.lowAction', { defaultValue: '去充值' });

  return (
    <div
      role='alert'
      aria-live='assertive'
      data-testid='sendbox-credits-bubble'
      className={classNames(
        'credits-bubble-card sendbox-credits-bubble credits-bubble-enter pl-12px pr-10px py-6px flex items-center gap-8px',
        isExhausted ? 'credits-bubble-card--exhausted' : 'credits-bubble-card--low'
      )}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => {
        // Prevent input textarea from blurring and tearing down the bubble mid-click
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <span
        className={classNames(
          'shrink-0 flex items-center justify-center size-14px rd-full',
          isExhausted ? 'text-[var(--danger)]' : 'text-[var(--flowy-attention)]'
        )}
      >
        <Attention theme='filled' size='13' fill='currentColor' />
      </span>

      <span className='text-12px font-500 leading-16px text-t-primary whitespace-nowrap'>
        {text}
      </span>

      <button
        type='button'
        className={classNames(
          'shrink-0 px-9px py-2px rd-5px text-11px font-600 border-none cursor-pointer transition-opacity hover:opacity-90 active:opacity-80',
          isExhausted
            ? 'bg-[var(--danger)] text-white'
            : 'bg-[var(--primary-6)] text-white'
        )}
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
        onClick={handleTopUp}
      >
        {actionText}
      </button>

      <button
        type='button'
        aria-label={t('common.creditsBubble.close', { defaultValue: '关闭提示' })}
        className='shrink-0 flex items-center justify-center size-16px rd-4px text-t-tertiary hover:text-t-primary hover:bg-fill-2 transition-colors border-none bg-transparent cursor-pointer p-0 ml-2px'
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
        onClick={handleDismiss}
      >
        <CloseSmall theme='outline' size='13' fill='currentColor' />
      </button>

      <div className='sendbox-credits-bubble-arrow-down' aria-hidden='true' />
    </div>
  );
};

export default SendBoxCreditsBubble;
