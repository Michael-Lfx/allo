/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
import type { ConversationArtifactId } from '@/common/types/conversationArtifact';
import type { ConversationId, CronJobId } from '@/common/types/ids';

import { ipcBridge } from '@/common';
import RecommendationCard from '@renderer/components/beautifulUi/recommendationCard/RecommendationCard';
import { toneFromSuggestion } from '@renderer/components/beautifulUi/recommendationCard/recommendationCardModel';
import { useUpdateConversationArtifactStatus } from '@renderer/pages/conversation/Messages/artifacts';
import { Message } from '@arco-design/web-react';
import { Down, Up } from '@icon-park/react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import MarkdownView from '@renderer/components/Markdown';
import type { SkillSuggestion } from '@renderer/utils/chat/skillSuggestParser';
import { MESSAGE_BODY_FONT_SIZE, MESSAGE_BODY_LINE_HEIGHT } from '../typography';
import styles from './SkillSuggestCard.module.css';

interface SkillSuggestCardProps {
  conversation_artifact_id: ConversationArtifactId;
  conversation_id: ConversationId;
  suggestion: SkillSuggestion;
  cron_job_id: CronJobId;
}

const CODE_STYLE = { marginTop: 4, marginBlock: 4 };

const SkillSuggestCard: React.FC<SkillSuggestCardProps> = ({
  conversation_artifact_id,
  conversation_id,
  suggestion,
  cron_job_id,
}) => {
  const { t } = useTranslation();
  const updateArtifactStatus = useUpdateConversationArtifactStatus();
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [expanded, setExpanded] = useState(false);

  // Check if skill already exists on mount (persists across navigation)
  useEffect(() => {
    ipcBridge.cron.hasSkill
      .invoke({ cron_job_id })
      .then((exists) => {
        if (exists) setSaved(true);
      })
      .catch(() => {});
  }, [cron_job_id]);

  if (dismissed || saved) return null;

  const handleSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await ipcBridge.cron.saveSkill.invoke({ cron_job_id, content: suggestion.content });
      updateArtifactStatus(conversation_artifact_id, 'saved');
      setSaved(true);
      Message.success(t('cron.skill.saveSuccess'));
    } catch (err) {
      Message.error(t('cron.skill.saveFailed'));
      console.error('[SkillSuggestCard] Failed to save skill:', err);
    } finally {
      setSaving(false);
    }
  };

  const handleDismiss = async () => {
    try {
      await ipcBridge.conversation.updateArtifact.invoke({
        conversation_id,
        conversation_artifact_id,
        status: 'dismissed',
      });
      updateArtifactStatus(conversation_artifact_id, 'dismissed');
      setDismissed(true);
    } catch (error) {
      Message.error(t('cron.skill.saveFailed'));
      console.error('[SkillSuggestCard] Failed to dismiss artifact:', error);
    }
  };

  return (
    <div data-testid='skill-suggest-card'>
      <RecommendationCard
        title={t('cron.skill.turnIntoSkill')}
        tone={toneFromSuggestion(suggestion)}
        body={
          <div className={styles.body}>
            <div className={styles.name}>{suggestion.name}</div>
            <div className={styles.description}>{suggestion.description}</div>
            <button type='button' className={styles.previewToggle} onClick={() => setExpanded(!expanded)}>
              {expanded ? <Up size={12} /> : <Down size={12} />}
              <span>{t('cron.skill.preview')}</span>
            </button>
            {expanded ? (
              <div className={styles.preview}>
                <MarkdownView
                  codeStyle={CODE_STYLE}
                  fontSize={MESSAGE_BODY_FONT_SIZE}
                  lineHeight={MESSAGE_BODY_LINE_HEIGHT}
                >
                  {`\`\`\`markdown\n${suggestion.content}\n\`\`\``}
                </MarkdownView>
              </div>
            ) : null}
          </div>
        }
        actions={[
          { id: 'accept', label: t('cron.skill.save'), onClick: () => void handleSave() },
          { id: 'dismiss', label: t('cron.skill.dismiss'), onClick: () => void handleDismiss() },
        ]}
      />
    </div>
  );
};

export default SkillSuggestCard;
