/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Attention } from '@icon-park/react';
import classNames from 'classnames';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
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
  const bubbleRef = useRef<HTMLDivElement>(null);

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

  // Click outside listener: dismiss when clicking elsewhere on the page
  useEffect(() => {
    if (bubbleState === 'normal') return;

    const handlePointerDown = (event: MouseEvent) => {
      if (bubbleRef.current && !bubbleRef.current.contains(event.target as Node)) {
        dismissCreditsBubble(userId, 'sendbox', bubbleState);
        setDismissedState(bubbleState);
      }
    };

    document.addEventListener('pointerdown', handlePointerDown);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
    };
  }, [bubbleState, userId]);

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

  const text = isExhausted
    ? t('conversation.sendBox.creditsExhaustedBlocked', {
        defaultValue: '积分已耗尽，请充值后发送',
      })
    : t('conversation.sendBox.creditsLowHint', {
        defaultValue: '积分不足 1,000，建议及时充值',
      });

  return (
    <div
      ref={bubbleRef}
      role='alert'
      aria-live='assertive'
      data-testid='sendbox-credits-bubble'
      className={classNames(
        'credits-bubble-card sendbox-credits-bubble credits-bubble-enter px-12px py-6px flex items-center gap-6px',
        isExhausted ? 'credits-bubble-card--exhausted' : 'credits-bubble-card--low'
      )}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => {
        // Prevent input textarea from blurring
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

      <span className='text-12px font-500 leading-16px text-t-primary whitespace-nowrap select-none'>
        {text}
      </span>

      <div className='sendbox-credits-bubble-arrow-down' aria-hidden='true' />
    </div>
  );
};

export default SendBoxCreditsBubble;
