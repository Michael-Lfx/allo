

import type { IMessageMoaReference } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import { BookOne, Right } from '@icon-park/react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './MessageMoaReference.module.css';

interface MessageMoaReferenceProps {
  message: IMessageMoaReference;
}

/**
 * Collapsible card for one MoA advisor suggestion. Collapsed by default; the
 * header shows the advisor label and index/total, the body shows the advice.
 */
const MessageMoaReference: React.FC<MessageMoaReferenceProps> = ({ message }) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const { label, index, total } = message.content;
  const text = toDisplayText(message.content.text);
  const positionText = index > 0 && total > 0 ? ` · ${index}/${total}` : '';
  const summaryText = `${t('messages.moa.referenceTitle', { defaultValue: 'Reference advice' })} · ${toDisplayText(label)}${positionText}`;

  return (
    <div className={styles.container}>
      <div className={styles.header} onClick={() => setExpanded((prev) => !prev)}>
        <span className={styles.headerIcon}>
          <BookOne theme='outline' size='14' />
        </span>
        <span className={styles.summary}>{summaryText}</span>
        <span className={`${styles.arrow} ${expanded ? styles.arrowExpanded : ''}`}>
          <Right theme='outline' size='12' />
        </span>
      </div>
      <div className={`${styles.body} ${!expanded ? styles.collapsed : ''}`}>{text}</div>
    </div>
  );
};

export default MessageMoaReference;
