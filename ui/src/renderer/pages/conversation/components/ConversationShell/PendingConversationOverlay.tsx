/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { Paperclip } from '@icon-park/react';
import classNames from 'classnames';
import { iconColors } from '@/renderer/styles/colors';
import { usePendingConversation } from './PendingConversationContext';
import styles from './PendingConversationOverlay.module.css';

/**
 * PendingConversationOverlay — the instant "creating conversation" transition.
 *
 * The moment the user sends from the Guid composer we cover the content region
 * with a conversation-shaped loading view: the just-sent message echoed as a
 * right-aligned user bubble (same skin/position as the real one) plus a left
 * "正在创建会话…" loading bubble. When the backend id arrives the flow seeds the
 * SWR cache and navigates to the real conversation, which renders the same user
 * bubble (via NomiSendBox's optimistic echo) in the same place — so uncovering
 * this overlay is seamless.
 *
 * Layout mirrors {@link ChatLayout} + {@link NomiChat}: a header-height top
 * spacer (min-h-44px + pt-8/pb-10 ≈ the real header) so the message area sits
 * at the same Y, a `px-20px` content column, and a composer-height bottom
 * spacer. Covers only the content region (mounted inside ConversationShell's
 * `relative` Outlet container), never the session sidebar.
 */
const PendingConversationOverlay: React.FC = () => {
  const { pending } = usePendingConversation();
  const { t } = useTranslation();

  if (!pending) return null;

  const caption = pending.sendsInitialMessage
    ? t('conversation.pending.creating', { defaultValue: '正在创建会话…' })
    : t('conversation.pending.startingAutoWork', { defaultValue: '正在启动 AutoWork…' });

  const fileCount = pending.files?.length ?? 0;
  const trimmedInput = pending.input.trim();
  const steps = [
    t('conversation.pending.stepValidate', { defaultValue: 'Validate' }),
    t('conversation.pending.stepCreate', { defaultValue: 'Create' }),
    t('conversation.pending.stepConfigure', { defaultValue: 'Configure' }),
    t('conversation.pending.stepOpen', { defaultValue: 'Open' }),
  ];
  const stageIndex = {
    validating: 0,
    creating: 1,
    configuring: 2,
    opening: 3,
  }[pending.stage ?? 'validating'];

  return (
    <div
      className={classNames('absolute inset-0 z-20 flex flex-col bg-1', styles.pendingOverlayEnter)}
      data-testid='pending-conversation-overlay'
      aria-busy='true'
    >
      {/* Header-height spacer — keeps the message area aligned with the real
          ChatLayout header so the swap doesn't jump vertically. */}
      <div className='shrink-0 min-h-44px pt-8px pb-10px' />

      <div className='flex-1 flex flex-col px-20px min-h-0 overflow-hidden'>
        <div className='flex-1 overflow-y-auto py-10px min-h-0'>
          {/* Echoed user message (right) — matches MessageText user bubble. */}
          {trimmedInput && (
            <div
              className={classNames(
                'w-full min-w-0 flex justify-end px-8px m-t-10px max-w-full md:max-w-780px mx-auto',
                styles.pendingUserBubbleEnter
              )}
            >
              <div className='min-w-0 flex flex-col items-end'>
                {fileCount > 0 && (
                  <div className='flex items-center gap-4px mb-6px text-t-secondary text-12px self-end'>
                    <Paperclip theme='outline' size='13' fill={iconColors.secondary} />
                    <span>{fileCount}</span>
                  </div>
                )}
                <div
                  className='min-w-0 bg-aou-2 p-6px md:p-8px md:max-w-780px whitespace-pre-wrap break-words'
                  style={{ borderRadius: '8px 0 8px 8px', color: 'var(--text-primary)' }}
                >
                  {trimmedInput}
                </div>
              </div>
            </div>
          )}

          {/* Preset loading bubble (left) — same skin as the skeleton bubbles. */}
          <div
            className={classNames(
              'w-full min-w-0 flex justify-start px-8px m-t-10px max-w-full md:max-w-780px mx-auto',
              styles.pendingAssistEnter
            )}
          >
            <div
              className='flex flex-col gap-10px rd-16px p-14px'
              style={{ background: 'var(--color-fill-1)', border: '1px solid var(--color-border-2)' }}
            >
              <div className='flex items-center gap-10px'>
                <Spin size={16} />
                <span className='text-t-primary text-14px leading-none'>{caption}</span>
              </div>
              <div className={styles.pendingSteps} aria-hidden='true'>
                {steps.map((label, index) => (
                  <React.Fragment key={label}>
                    {index > 0 ? <span className={styles.pendingStepArrow}>→</span> : null}
                    <span
                      className={classNames(
                        styles.pendingStep,
                        index === stageIndex && styles.pendingStepActive,
                        index < stageIndex && styles.pendingStepDone
                      )}
                    >
                      {label}
                    </span>
                  </React.Fragment>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Composer-height spacer so the layout footprint matches the real page. */}
        <div className='shrink-0 h-84px' />
      </div>
    </div>
  );
};

export default PendingConversationOverlay;
