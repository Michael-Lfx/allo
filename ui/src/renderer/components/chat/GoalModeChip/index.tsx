import type { ConversationId } from '@/common/types/ids';
import { useGoalCommand, useGoalStatus } from '@/renderer/hooks/chat/useGoalCommand';
import { Button, Popconfirm } from '@arco-design/web-react';
import { Aiming, CloseSmall } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './index.module.css';

/**
 * Compact "目标" chip for the conversation composer, mirroring the home page's
 * goal-mode chip (GuidActionRow). The controlled `armed` state covers the
 * interval after `/goal` is chosen and before the objective is submitted; an
 * active persisted goal continues to use the live goal snapshot.
 */
const GoalModeChip: React.FC<{
  conversation_id: ConversationId;
  armed?: boolean;
  onArmedChange?: (armed: boolean) => void;
}> = ({ conversation_id, armed = false, onArmedChange }) => {
  const { t } = useTranslation();
  const { goal } = useGoalStatus(conversation_id);
  const goalCommand = useGoalCommand(conversation_id);

  // 后端快照的 `active` 对 cleared/complete/blocked 终端状态仍为 true；chip 只在
  // 目标仍在续作（active/paused/waiting）时显示，清除/完成后随即消失（终端状态
  // 由 GoalStatusNotice 状态卡片呈现）。
  const hasActiveGoal =
    goal?.active === true &&
    (goal.status === 'active' || goal.status === 'paused' || goal.status === 'waiting');

  if (!armed && !hasActiveGoal) {
    return null;
  }

  const clearsPersistedGoal = hasActiveGoal && !armed;
  const chip = (
    <Button
      type='text'
      shape='round'
      size='small'
      className={`${styles.chip} flowy-icon-text-btn`}
      aria-label={t('conversation.goal.chip.clearAria', { defaultValue: 'Clear goal mode' })}
      data-testid='goal-mode-chip'
      onClick={clearsPersistedGoal ? undefined : () => onArmedChange?.(false)}
    >
      <span className={`${styles.chipContent} flowy-button-inline-content`}>
        <span className={styles.chipIcon} aria-hidden='true'>
          <span className={styles.chipMark}>
            <Aiming theme='outline' size='16' strokeWidth={3} fill='currentColor' />
          </span>
          <span className={styles.chipClose}>
            <CloseSmall theme='outline' size='12' strokeWidth={5} fill='currentColor' />
          </span>
        </span>
        <span className={styles.chipLabel}>{t('conversation.goal.chip.label', { defaultValue: 'Goal' })}</span>
      </span>
    </Button>
  );

  if (!clearsPersistedGoal) {
    return chip;
  }

  return (
    <Popconfirm title={t('conversation.goal.notice.clearConfirm')} onOk={() => void goalCommand.run({ action: 'clear' })}>
      {chip}
    </Popconfirm>
  );
};

export default GoalModeChip;
