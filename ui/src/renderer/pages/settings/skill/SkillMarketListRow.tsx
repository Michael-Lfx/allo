import { Button, Tag } from '@arco-design/web-react';
import { Check, LinkOne, Plus } from '@icon-park/react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { MarketActionState } from './marketContracts';
import { formatSkillHubMarketCount } from './marketViewModel';
import type { MarketItemViewModel } from './marketViewModel';
import { normalizeTestId } from './skillPresentation';

type SkillMarketListRowProps = {
  item: MarketItemViewModel;
  actionLabel: string;
  pendingLabel: string;
  completedLabel?: string;
  actionState?: MarketActionState;
  busy?: boolean;
  disabled?: boolean;
  onAdd: (item: MarketItemViewModel) => void;
  onOpenSource: (item: MarketItemViewModel) => void;
  onViewDetails: (item: MarketItemViewModel, trigger?: HTMLElement) => void;
};

const sourceLabel = (item: MarketItemViewModel): string | null => {
  if (item.marketSource === 'clawhub') return 'ClawHub';
  if (item.marketSource === 'skillhub') return 'SkillHub';
  return item.upstreamSource || null;
};

const SkillMarketListRow: React.FC<SkillMarketListRowProps> = ({
  item,
  actionLabel,
  pendingLabel,
  completedLabel,
  actionState = 'ready',
  busy = false,
  disabled = false,
  onAdd,
  onOpenSource,
  onViewDetails,
}) => {
  const { t, i18n } = useTranslation();
  const [avatarBroken, setAvatarBroken] = useState(false);
  const isCompleted = actionState === 'completed';
  const isChecking = actionState === 'checking';
  const actionText = isChecking
    ? t('common.loading', { defaultValue: '加载中' })
    : isCompleted
      ? completedLabel ?? actionLabel
      : busy
        ? pendingLabel
        : actionLabel;

  useEffect(() => setAvatarBroken(false), [item.avatar]);

  return (
    <article
      className='grid min-w-0 grid-cols-1 items-center gap-10px rounded-16px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-14px py-12px transition-colors duration-150 hover:border-[var(--color-border-3)] motion-reduce:transition-none sm:grid-cols-[auto_minmax(0,1fr)_auto] sm:gap-14px'
      data-testid={`skill-market-list-row-${normalizeTestId(item.id)}`}
    >
      <span className='flex h-44px w-44px shrink-0 items-center justify-center overflow-hidden rounded-10px bg-[var(--color-fill-2)] text-12px font-semibold text-t-secondary'>
        {item.avatar && !avatarBroken ? (
          <img src={item.avatar} alt='' width={44} height={44} referrerPolicy='no-referrer' className='h-44px w-44px object-contain' onError={() => setAvatarBroken(true)} />
        ) : (
          <span aria-hidden='true'>–</span>
        )}
      </span>

      <div className='min-w-0'>
        <div className='flex min-w-0 flex-wrap items-center gap-x-8px gap-y-4px'>
          <h3 className='m-0 min-w-0 truncate text-15px font-semibold text-t-primary' title={item.title}>{item.title}</h3>
          {item.skillHub && <span className='shrink-0 text-11px text-t-tertiary'>v{item.skillHub.version}</span>}
          <span className='shrink-0 text-12px text-t-tertiary'>
            {sourceLabel(item) || t('settings.skillsMarket.sourceUnknown', { defaultValue: '来源未知' })}
          </span>
          {item.requiresApi && (
            <Tag size='small' bordered={false} className='!rounded-6px !bg-fill-2 !text-10px !text-t-secondary'>
              {t('settings.skillsMarket.requiresApi', { defaultValue: '需要 API Key' })}
            </Tag>
          )}
        </div>
        <p className='m-0 mt-4px overflow-hidden text-13px leading-20px text-t-secondary' title={item.fullDescription} style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}>
          {item.summary || t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' })}
        </p>
        {item.skillHub && (
          <div className='mt-6px flex min-w-0 flex-wrap gap-x-8px gap-y-4px text-12px text-t-tertiary'>
            {item.skillHub.category && <span className='truncate'>{item.skillHub.category}</span>}
            <span title={item.skillHub.downloads.toLocaleString(i18n.language)}>{formatSkillHubMarketCount(item.skillHub.downloads)} {t('settings.skillsMarket.downloadUnit', { defaultValue: '下载' })}</span>
            <span>{t('settings.skillsMarket.score', { score: item.skillHub.score.toFixed(1), defaultValue: '热度分 {{score}}' })}</span>
          </div>
        )}
      </div>

      <div className='col-start-1 flex shrink-0 items-center justify-self-end gap-6px sm:col-auto'>
        <Button
          type='text'
          size='small'
          className='!h-32px !rounded-8px !px-8px !text-t-secondary hover:!text-t-primary'
          onClick={(event) => onViewDetails(item, event.currentTarget as HTMLElement)}
        >
          {t('settings.market.viewDetails', { defaultValue: '查看详情' })}
        </Button>
        <Button
          size='small'
          type={isCompleted ? 'outline' : 'primary'}
          loading={busy || isChecking}
          disabled={disabled}
          className='flowy-icon-text-btn !h-32px !min-w-88px !rounded-8px !px-12px !whitespace-nowrap'
          icon={isCompleted ? <Check theme='outline' size={12} strokeWidth={3} fill='currentColor' /> : !busy && !isChecking ? <Plus theme='outline' size={12} strokeWidth={3} fill='currentColor' /> : undefined}
          onClick={() => onAdd(item)}
        >
          {actionText}
        </Button>
        <Button type='text' size='small' className='!h-32px !w-32px !rounded-8px !p-0 !text-t-secondary' aria-label={t('settings.market.openSource', { defaultValue: '打开来源' })} onClick={() => onOpenSource(item)} icon={<LinkOne size={14} fill='currentColor' />} />
      </div>
    </article>
  );
};

export default SkillMarketListRow;
