import type { ConversationId } from '@/common/types/ids';
import { useGoalCommand, useGoalStatus } from '@/renderer/hooks/chat/useGoalCommand';
import { Button, Popconfirm } from '@arco-design/web-react';
import { Aiming, CloseSmall } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './index.module.css';

/**
 * Compact "目标" chip for the conversation composer, mirroring the home page's
 * goal-mode chip (GuidActionRow). Unlike the home page — where `/goal` only
 * arms the first message — the conversation is driven by the live goal
 * snapshot: the chip appears while the conversation has an active goal and
 * clears it (goal + subgoals + contract) through the same API the status card
 * uses. Renders nothing when the conversation has no goal.
 */
const GoalModeChip: React.FC<{ conversation_id: ConversationId }> = ({ conversation_id }) => {
  const { t } = useTranslation();
  const { goal } = useGoalStatus(conversation_id);
  const goalCommand = useGoalCommand(conversation_id);

  // 后端快照的 `active` 对 cleared/complete/blocked 终端状态仍为 true；chip 只在
  // 目标仍在续作（active/paused/waiting）时显示，清除/完成后随即消失（终端状态
  // 由 GoalStatusNotice 状态卡片呈现）。
  const isChipVisible =
    goal?.active === true &&
    (goal.status === 'active' || goal.status === 'paused' || goal.status === 'waiting');

  if (!isChipVisible) {
    return null;
  }

  return (
    <Popconfirm
      title={t('conversation.goal.notice.clearConfirm')}
      onOk={() => void goalCommand.run({ action: 'clear' })}
    >
      <Button
        type='text'
        shape='round'
        size='small'
        className={styles.chip}
        aria-label={t('conversation.goal.chip.clearAria', { defaultValue: 'Clear goal mode' })}
        data-testid='goal-mode-chip'
      >
        <span className={styles.chipContent}>
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
    </Popconfirm>
  );
};

export default GoalModeChip;
