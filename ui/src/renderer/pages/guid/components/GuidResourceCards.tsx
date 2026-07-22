/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Robot, People, ApiApp } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import styles from '../index.module.css';

const PathCard: React.FC<{
  icon: React.ReactNode;
  title: string;
  description: string;
  action: string;
  onClick: () => void;
  testId: string;
}> = ({ icon, title, description, action, onClick, testId }) => (
  <button type='button' className={styles.guidResourceCard} onClick={onClick} data-testid={testId}>
    <span className={styles.guidResourceCardHeader}>
      <span className={styles.guidResourceIcon}>{icon}</span>
      <span className={styles.guidResourceTitle}>{title}</span>
    </span>
    <span className={styles.guidResourceDescription}>{description}</span>
    <span className={styles.guidResourceAction}>{action}</span>
  </button>
);

type GuidResourceCardsProps = {
  onStartLocalAgent?: () => void;
};

/**
 * Guid empty-area: three in-app demo paths only (local agent, companion remote, mcp-agent).
 */
const GuidResourceCards: React.FC<GuidResourceCardsProps> = ({ onStartLocalAgent }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();

  return (
    <div className={styles.guidResourceCards} data-testid='guid-resource-cards'>
      <PathCard
        testId='guid-path-local-agent'
        icon={<Robot theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.localAgentTitle', {
          defaultValue: 'Local agent work',
        })}
        description={t('conversation.emptyCards.localAgentDescription', {
          defaultValue: 'Day-1 path: pick a Preset or Agent below, send a task, watch the session loop.',
        })}
        action={t('conversation.emptyCards.localAgentAction', { defaultValue: 'Start below' })}
        onClick={() => onStartLocalAgent?.()}
      />
      <PathCard
        testId='guid-path-companion'
        icon={<People theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.companionTitle', {
          defaultValue: 'Companion remote control',
        })}
        description={t('conversation.emptyCards.companionDescription', {
          defaultValue: 'Bind a channel, then drive this machine from the first IM message.',
        })}
        action={t('conversation.emptyCards.companionAction', { defaultValue: 'Open companions' })}
        onClick={() => void navigate('/nomi')}
      />
      <PathCard
        testId='guid-path-open-caps'
        icon={<ApiApp theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.openCapsTitle', {
          defaultValue: 'External agents on /mcp-agent',
        })}
        description={t('conversation.emptyCards.openCapsDescription', {
          defaultValue: 'Open the port, mint a token, paste Cursor/Claude config, call one tool.',
        })}
        action={t('conversation.emptyCards.openCapsAction', { defaultValue: 'Open Capabilities' })}
        onClick={() => void navigate('/open-capabilities')}
      />
    </div>
  );
};

export default GuidResourceCards;
