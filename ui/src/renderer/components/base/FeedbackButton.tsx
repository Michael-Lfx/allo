

import { useFeedback } from '@/renderer/hooks/context/FeedbackContext';
import type { ConversationErrorReportContext } from '@/renderer/features/supportChat/conversationErrorReport';
import { Comment } from '@icon-park/react';
import classNames from 'classnames';
import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

type FeedbackButtonProps = {
  /** Pre-selects the module in the feedback modal (see FEEDBACK_MODULES tags). */
  module?: string;
  /** Extra Sentry tags attached to the feedback event. */
  feedbackTags?: Record<string, string>;
  /** Extra structured context attached to the feedback event. */
  feedbackExtra?: Record<string, unknown>;
  /** Sends this conversation error and recent logs through support IM after confirmation. */
  conversationErrorReport?: ConversationErrorReportContext;
  /** Additional classes appended to the default pill styling. */
  className?: string;
};

/**
 * Inline feedback chip shown near error messages — styled as a compact pill
 * consistent with Nomi's existing Mention/Agent pill patterns. Click
 * auto-captures the current window and opens the feedback modal with the
 * relevant module preselected; the user only needs to describe the issue.
 */
const FeedbackButton: React.FC<FeedbackButtonProps> = ({
  module,
  feedbackTags,
  feedbackExtra,
  conversationErrorReport,
  className,
}) => {
  const { t } = useTranslation();
  const { openFeedback } = useFeedback();

  const handleClick = useCallback(
    (event: React.MouseEvent<HTMLElement>) => {
      event.stopPropagation();
      openFeedback({
        module,
        autoScreenshot: !conversationErrorReport,
        tags: feedbackTags,
        extra: feedbackExtra,
        conversationErrorReport,
      }).catch((err) => {
        console.error('[FeedbackButton] Failed to open feedback:', err);
      });
    },
    [conversationErrorReport, feedbackExtra, feedbackTags, module, openFeedback]
  );

  return (
    <button
      type='button'
      role='button'
      onClick={handleClick}
      className={classNames(
        'inline-flex items-center gap-3px cursor-pointer select-none b-none',
        'px-8px py-4px rd-16px',
        'bg-transparent hover:bg-fill-2 text-t-primary',
        'text-13px leading-18px transition-colors duration-150',
        className
      )}
    >
      <Comment theme='outline' size='14' fill='currentColor' className='flex-shrink-0 pt-4px' />
      <span>{t('settings.oneClickFeedback')}</span>
    </button>
  );
};

export default FeedbackButton;
