

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { Paperclip } from '@icon-park/react';
import classNames from 'classnames';
import { iconColors } from '@/renderer/styles/colors';
import {
  CHAT_COMPOSER_WRAPPER_CLASSES,
  CHAT_CONTENT_COLUMN_CLASSES,
  CHAT_HEADER_CLASSES,
  CHAT_HEADER_WITH_SUBTITLE_CLASSES,
  getWorkspaceTitleSubtitle,
  CHAT_MESSAGE_ROW_METRICS_CLASSES,
  CHAT_SCROLL_AREA_CLASSES,
} from '@/renderer/pages/conversation/components/conversationLayoutClasses';
import PathText from '@/renderer/components/base/PathText';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import appLogo from '@/renderer/assets/logo.svg';
import { usePendingConversation } from './PendingConversationContext';
import styles from './PendingConversationOverlay.module.css';

/**
 * PendingConversationOverlay — a high-fidelity skeleton of the real
 * conversation page, shown the instant the user sends from the Guid composer
 * and uncovered (crossfaded) once the destination mounts.
 *
 * It is a static replica of {@link ChatLayout}: the header (logo + any
 * explicitly provided conversation context), the content column with the just-sent user
 * bubble (same skin/position as MessageText) plus a borderless left-aligned
 * assistant loading line (matching real assistant text — no card), a static
 * composer shell at the same height, and the 32px right tool-rail. Every metric
 * is shared with the real components via `conversationLayoutClasses`, so the
 * reveal is a crossfade rather than a reflow. The overlay is decorative — it
 * owns no handlers and is aria-hidden where it has no interactive counterpart.
 */
const PendingConversationOverlay: React.FC = () => {
  const { pending, phase } = usePendingConversation();
  const { t } = useTranslation();
  const isMobile = Boolean(useLayoutContext()?.isMobile);

  if (!pending) return null;

  const caption = pending.sendsInitialMessage
    ? t('conversation.pending.creating', { defaultValue: '正在创建会话…' })
    : t('conversation.pending.startingAutoWork', { defaultValue: '正在启动 AutoWork…' });

  const fileCount = pending.files?.length ?? 0;
  const trimmedInput = pending.input.trim();
  // The first user message is a preview only. The backend owns the formal
  // fallback title and writes it after the message is durable.
  const displayTitle = pending.title?.trim() || '';
  const workspaceTitleSubtitle = getWorkspaceTitleSubtitle(pending.workspacePath, isMobile);
  // Mirrors NomiSendBox's placeholder contract. backend stays 'Flowy' (the
  // overlay renders before the destination, so the real agent_name isn't known
  // yet) — the common case matches exactly; a custom-named agent differs by one
  // word during the ~140ms crossfade. The defaultValue uses the same {{backend}}
  // template so the missing-key fallback still interpolates.
  const composerPlaceholder = t('acp.sendbox.placeholder', {
    backend: 'Flowy',
    defaultValue: 'Send message to {{backend}}...',
  });

  return (
    <div
      className={classNames(
        'absolute inset-0 z-20 flex bg-1',
        // Fading out: let clicks reach the revealed conversation immediately.
        phase === 'exiting' && 'pointer-events-none',
        phase === 'exiting' ? styles.pendingOverlayExit : styles.pendingOverlayEnter
      )}
      data-testid='pending-conversation-overlay'
      aria-busy='true'
    >
      {/* Main replica column — mirrors ChatLayout's chat column (header +
          content + composer). Matching its metrics makes the reveal a
          crossfade, not a reflow. */}
      <div className='flex-1 min-w-0 flex flex-col'>
        {/* Header replica (CHAT_HEADER_CLASSES): only explicit context is shown
            here; a normal message must never be mistaken for a formal title. */}
        <div
          className={classNames(
            CHAT_HEADER_CLASSES,
            workspaceTitleSubtitle && CHAT_HEADER_WITH_SUBTITLE_CLASSES,
          )}
        >
          <div className='flex items-center min-w-0'>
            <div className='shrink-0 flex items-center pl-8px'>
              <img src={appLogo} alt='Flowy' className='block h-16px w-16px object-contain' />
            </div>
            <div className='min-w-0 flex-1 flex flex-col justify-center px-8px py-5px'>
              <span className='block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-16px font-bold text-t-primary'>
                {displayTitle || ' '}
              </span>
              {workspaceTitleSubtitle && (
                <span
                  data-testid='conversation-workspace-subtitle'
                  title={workspaceTitleSubtitle}
                  className='block min-w-0 overflow-hidden text-11px leading-14px text-t-secondary'
                >
                  <PathText path={workspaceTitleSubtitle} className='min-w-0' />
                </span>
              )}
            </div>
          </div>
        </div>

        <div className={classNames(CHAT_CONTENT_COLUMN_CLASSES, 'overflow-hidden')}>
          <div className={CHAT_SCROLL_AREA_CLASSES}>
            {/* Echoed user message (right) — matches MessageText's user bubble
                (padding/radius/bg), so it sits identically under the real one. */}
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
                    className='min-w-0 bg-aou-2 px-10px py-7px md:px-12px md:py-9px md:max-w-780px whitespace-pre-wrap break-words'
                    style={{ borderRadius: '14px 5px 14px 14px', color: 'var(--text-primary)' }}
                  >
                    {trimmedInput}
                  </div>
                </div>
              </div>
            )}

            {/* Assistant loading (left) — borderless, no avatar/dot, sharing the
                same X start as a real assistant text message so the thinking
                line doesn't resolve into a differently-shaped card at reveal. */}
            <div
              className={classNames(
                'w-full min-w-0 flex justify-start',
                CHAT_MESSAGE_ROW_METRICS_CLASSES,
                styles.pendingAssistEnter
              )}
            >
              <div className='min-w-0 w-full flex flex-col items-start'>
                <div className='flex items-center gap-10px min-h-20px'>
                  <Spin size={16} />
                  <span className='text-t-secondary text-14px leading-none'>{caption}</span>
                </div>
              </div>
            </div>
          </div>

          {/* Static composer replica — same wrapper/panel/toolbar footprint as
              the real SendBox so the page's bottom edge doesn't shift at
              reveal. Decorative only. */}
          <div className={CHAT_COMPOSER_WRAPPER_CLASSES} aria-hidden='true'>
            <div
              className='flex flex-col p-16px rd-20px bg-dialog-fill-0'
              style={{ border: '1px solid var(--color-border-2)' }}
            >
              <div
                className='h-40px mb-8px text-14px text-t-secondary overflow-hidden'
                style={{ lineHeight: '20px' }}
              >
                {composerPlaceholder}
              </div>
              <div className='flex items-center justify-between gap-2 w-full min-h-28px'>
                <span className='inline-flex w-28px h-28px rd-full bg-fill-2 shrink-0' />
                <span className='inline-flex w-26px h-26px rd-full bg-primary-6 shrink-0' />
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Right tool-rail replica (desktop only) — 32px column matching
          WorkspaceToolRail, so the overlay occupies the same right edge the
          real page will, instead of leaving a blank strip that pops at reveal. */}
      {!isMobile && (
        <div
          className='w-32px shrink-0 flex flex-col items-center gap-3px bg-1'
          style={{ padding: '8px 2px', borderLeft: '1px solid var(--bg-3)' }}
          aria-hidden='true'
        >
          <span className='w-28px h-28px rd-8px bg-fill-2' />
          <span className='w-28px h-28px rd-8px bg-fill-2' />
          <span className='w-28px h-28px rd-8px bg-fill-2 mt-auto' />
        </div>
      )}
    </div>
  );
};

export default PendingConversationOverlay;
