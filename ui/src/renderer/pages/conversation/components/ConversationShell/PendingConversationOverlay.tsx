

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { Paperclip } from '@icon-park/react';
import classNames from 'classnames';
import { iconColors } from '@/renderer/styles/colors';
import {
  CHAT_COMPOSER_SPACER_CLASSES,
  CHAT_CONTENT_COLUMN_CLASSES,
  CHAT_HEADER_SPACER_CLASSES,
  CHAT_MESSAGE_ROW_METRICS_CLASSES,
  CHAT_SCROLL_AREA_CLASSES,
} from '@/renderer/pages/conversation/components/conversationLayoutClasses';
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
  const { pending, phase } = usePendingConversation();
  const { t } = useTranslation();

  if (!pending) return null;

  const caption = pending.sendsInitialMessage
    ? t('conversation.pending.creating', { defaultValue: '正在创建会话…' })
    : t('conversation.pending.startingAutoWork', { defaultValue: '正在启动 AutoWork…' });

  const fileCount = pending.files?.length ?? 0;
  const trimmedInput = pending.input.trim();
  const steps = [
    t('conversation.pending.stepValidate', { defaultValue: '核对任务' }),
    t('conversation.pending.stepCreate', { defaultValue: '创建会话' }),
    t('conversation.pending.stepConfigure', { defaultValue: '准备工具' }),
    t('conversation.pending.stepOpen', { defaultValue: '开始执行' }),
  ];
  const stageIndex = {
    validating: 0,
    creating: 1,
    configuring: 2,
    opening: 3,
  }[pending.stage ?? 'validating'];

  return (
    <div
      className={classNames(
        'absolute inset-0 z-20 flex flex-col bg-1',
        // Fading out: let clicks reach the revealed conversation immediately.
        phase === 'exiting' && 'pointer-events-none',
        phase === 'exiting' ? styles.pendingOverlayExit : styles.pendingOverlayEnter
      )}
      data-testid='pending-conversation-overlay'
      aria-busy='true'
    >
      {/* Header-height spacer — keeps the message area aligned with the real
          ChatLayout header so the swap doesn't jump vertically. */}
      <div className={CHAT_HEADER_SPACER_CLASSES} />

      <div className={classNames(CHAT_CONTENT_COLUMN_CLASSES, 'overflow-hidden')}>
        <div className={CHAT_SCROLL_AREA_CLASSES}>
          {/* Echoed user message (right) — matches MessageText user bubble. */}
          {trimmedInput && (
            <div
              className={classNames(
                'w-full min-w-0 flex justify-end',
                CHAT_MESSAGE_ROW_METRICS_CLASSES,
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
              'w-full min-w-0 flex justify-start',
              CHAT_MESSAGE_ROW_METRICS_CLASSES,
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
        <div className={CHAT_COMPOSER_SPACER_CLASSES} />
      </div>
    </div>
  );
};

export default PendingConversationOverlay;
