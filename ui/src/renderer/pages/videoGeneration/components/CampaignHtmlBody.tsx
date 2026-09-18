
import React, { useCallback } from 'react';
import { openExternalUrl } from '@renderer/utils/platform';
import { sanitizeCampaignHtml } from '../campaign';
import styles from '../campaign.module.css';

interface CampaignHtmlBodyProps {
  html: string;
}

const CampaignHtmlBody: React.FC<CampaignHtmlBodyProps> = ({ html }) => {
  const safe = sanitizeCampaignHtml(html);

  const onClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement | null;
    const anchor = target?.closest('a');
    if (!anchor) return;
    const href = anchor.getAttribute('href');
    if (!href) return;
    if (/^(https?:|mailto:)/i.test(href)) {
      event.preventDefault();
      void openExternalUrl(href).catch((error) => {
        console.warn('[videoGeneration] campaign html link failed', error);
      });
    }
  }, []);

  return (
    <div
      className={styles.htmlBody}
      onClick={onClick}
      dangerouslySetInnerHTML={{ __html: safe }}
    />
  );
};

export default CampaignHtmlBody;
