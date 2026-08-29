
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag } from '@arco-design/web-react';
import { Trophy } from '@icon-park/react';
import type { CampaignPhase, CampaignSummary } from '../types';
import { formatCampaignRange } from '../campaign';
import styles from '../index.module.css';
import campaignStyles from '../campaign.module.css';

function phaseColor(phase: CampaignPhase | undefined): string {
  switch (phase) {
    case 'ongoing':
      return 'green';
    case 'upcoming':
      return 'arcoblue';
    case 'ended':
      return 'gray';
    default:
      return 'gray';
  }
}

interface CampaignCardProps {
  campaign: CampaignSummary;
  onOpen: (campaign: CampaignSummary) => void;
}

const CampaignCard: React.FC<CampaignCardProps> = ({ campaign, onOpen }) => {
  const { t, i18n } = useTranslation();
  const range = useMemo(
    () => formatCampaignRange(campaign.startAt, campaign.endAt, i18n.language),
    [campaign.endAt, campaign.startAt, i18n.language]
  );
  const phaseLabel = t(`videoGeneration.campaign.phase.${campaign.phase}`, {
    defaultValue:
      campaign.phase === 'ongoing'
        ? '进行中'
        : campaign.phase === 'upcoming'
          ? '未开始'
          : campaign.phase === 'ended'
            ? '已结束'
            : campaign.phase,
  });

  return (
    <div
      role='button'
      tabIndex={0}
      className={[
        styles.projectCard,
        'group relative flex flex-col overflow-hidden box-border cursor-pointer',
      ].join(' ')}
      onClick={() => onOpen(campaign)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(campaign);
        }
      }}
    >
      <div className={`${styles.projectCover} ${campaignStyles.cardCover} relative overflow-hidden`}>
        {campaign.coverUrl ? (
          <img
            src={campaign.coverUrl}
            alt=''
            className={styles.projectCoverMedia}
            draggable={false}
            loading='lazy'
            decoding='async'
          />
        ) : (
          <div className={styles.projectCoverFallback}>
            <span className='flex h-28px w-28px items-center justify-center rd-8px border border-solid border-[rgba(var(--primary-6),0.2)] bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'>
              <Trophy theme='outline' size={15} fill='currentColor' />
            </span>
          </div>
        )}
        <div className={styles.projectCoverOverlay}>
          <Tag size='small' color={phaseColor(campaign.phase)} className={campaignStyles.phaseTag}>
            {phaseLabel}
          </Tag>
        </div>
      </div>
      <div className='flex flex-col gap-6px p-14px'>
        <div className='truncate text-15px font-600 leading-[1.3] text-[var(--color-text-1)]'>
          {campaign.title}
        </div>
        {campaign.summary ? (
          <div className={campaignStyles.cardSummary}>{campaign.summary}</div>
        ) : null}
        <div className='flex items-center justify-between gap-8px'>
          <div className='truncate text-11px text-[var(--color-text-4)]'>{range}</div>
          {campaign.canSubmit ? (
            <Tag size='small' color='orangered' className='shrink-0'>
              {t('videoGeneration.campaign.canSubmit', { defaultValue: '可投稿' })}
            </Tag>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export default React.memo(CampaignCard);
export { phaseColor };
