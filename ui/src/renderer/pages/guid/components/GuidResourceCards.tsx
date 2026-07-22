/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { BookOne, Comment, PlayOne, Afferent } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { openExternalUrl } from '@/renderer/utils/platform';
import styles from '../index.module.css';

const DOCS_URL = 'https://www.nomifun.com/docs';
const VIDEO_URL_CN = 'https://www.bilibili.com/video/BV1kwKZ6UE5X/';
const VIDEO_URL_GLOBAL = 'https://youtu.be/AsEToBDFR9s';
const FEEDBACK_URL = 'https://www.nomifun.com/contact';

const ResourceLinkCard: React.FC<{
  icon: React.ReactNode;
  title: string;
  description: string;
  action: string;
  onClick: () => void;
}> = ({ icon, title, description, action, onClick }) => (
  <button type='button' className={styles.guidResourceCard} onClick={onClick}>
    <span className={styles.guidResourceCardHeader}>
      <span className={styles.guidResourceIcon}>{icon}</span>
      <span className={styles.guidResourceTitle}>{title}</span>
    </span>
    <span className={styles.guidResourceDescription}>{description}</span>
    <span className={styles.guidResourceAction}>{action}</span>
  </button>
);

/**
 * Guid empty-area cards. Includes an in-app knowledge demo path
 * (bind KB via preset → writeback → inbox) plus legacy external links.
 */
const GuidResourceCards: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const language = i18n.resolvedLanguage || i18n.language;
  const videoUrl = language.toLowerCase().startsWith('zh') ? VIDEO_URL_CN : VIDEO_URL_GLOBAL;

  return (
    <div className={styles.guidResourceCards} data-testid='guid-resource-cards'>
      <ResourceLinkCard
        icon={<Afferent theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.knowledgeTitle', {
          defaultValue: 'Knowledge writeback demo',
        })}
        description={t('conversation.emptyCards.knowledgeDescription', {
          defaultValue: 'Create or sync a base, attach it in a Preset (staged writeback), then review inbox merges.',
        })}
        action={t('conversation.emptyCards.knowledgeAction', { defaultValue: 'Open knowledge' })}
        onClick={() => void navigate('/knowledge')}
      />
      <ResourceLinkCard
        icon={<BookOne theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.docsTitle')}
        description={t('conversation.emptyCards.docsDescription')}
        action={t('conversation.emptyCards.docsAction')}
        onClick={() => void openExternalUrl(DOCS_URL)}
      />
      <ResourceLinkCard
        icon={<PlayOne theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.videoTitle')}
        description={t('conversation.emptyCards.videoDescription')}
        action={t('conversation.emptyCards.videoAction')}
        onClick={() => void openExternalUrl(videoUrl)}
      />
      <ResourceLinkCard
        icon={<Comment theme='outline' size='18' fill='currentColor' />}
        title={t('conversation.emptyCards.feedbackTitle')}
        description={t('conversation.emptyCards.feedbackDescription')}
        action={t('conversation.emptyCards.feedbackAction')}
        onClick={() => void openExternalUrl(FEEDBACK_URL)}
      />
    </div>
  );
};

export default GuidResourceCards;
