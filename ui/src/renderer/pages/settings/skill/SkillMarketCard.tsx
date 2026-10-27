import { Button, Dropdown, Menu, Tag } from '@arco-design/web-react';
import { Check, LinkOne, More, Plus } from '@icon-park/react';
import React, { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { normalizeTestId } from './skillPresentation';
import MarketCardShell from './MarketCardShell';
import type { MarketActionState } from './marketContracts';
import { formatSkillHubMarketCount } from './marketViewModel';
import type { MarketItemViewModel } from './marketViewModel';

type SkillMarketCardProps = {
  item: MarketItemViewModel;
  actionLabel: string;
  pendingLabel: string;
  completedLabel?: string;
  actionState?: MarketActionState;
  busy?: boolean;
  disabled?: boolean;
  showInstallCommand?: boolean;
  onAdd: (item: MarketItemViewModel) => void;
  onOpenSource: (item: MarketItemViewModel) => void;
  onCopyInstallCommand: (item: MarketItemViewModel) => void;
  onViewDetails: (item: MarketItemViewModel, trigger?: HTMLElement) => void;
};

const MAX_VISIBLE_TAGS = 2;

/**
 * A market item is an article, not a giant button. Details and operations are
 * explicit actions so keyboard users can understand and reach every affordance.
 */
const SkillMarketCard: React.FC<SkillMarketCardProps> = ({
  item,
  actionLabel,
  pendingLabel,
  completedLabel,
  actionState = 'ready',
  busy = false,
  disabled = false,
  showInstallCommand = true,
  onAdd,
  onOpenSource,
  onCopyInstallCommand,
  onViewDetails,
}) => {
  const { t, i18n } = useTranslation();
  const testId = normalizeTestId(item.id);
  const moreButtonRef = useRef<HTMLButtonElement>(null);
  const [brokenAvatar, setBrokenAvatar] = useState<string | undefined>();
  const avatarSrc = item.avatar && brokenAvatar !== item.avatar ? item.avatar : undefined;
  const isCompleted = actionState === 'completed';
  const isChecking = actionState === 'checking';
  let actionText = actionLabel;
  if (isChecking) {
    actionText = t('common.loading', { defaultValue: '加载中' });
  } else if (isCompleted) {
    actionText = completedLabel ?? actionLabel;
  } else if (busy) {
    actionText = pendingLabel;
  }

  return (
    <MarketCardShell reflective testId={`skill-market-card-${testId}`}>
      <header className='flex items-center gap-8px text-12px leading-18px'>
        <span className='inline-flex min-w-0 items-center gap-4px font-medium text-t-primary'>
          <Check theme='outline' size={14} fill='currentColor' />
          <span className='truncate'>
            {item.marketSource === 'clawhub'
              ? 'ClawHub'
              : item.marketSource === 'skillhub'
                ? 'SkillHub'
                : item.upstreamSource || t('settings.skillsMarket.sourceUnknown', { defaultValue: '来源未知' })}
          </span>
        </span>
        {isCompleted ? (
          <span className='ml-auto inline-flex items-center gap-6px text-t-tertiary'>
            <span className='h-6px w-6px rounded-full bg-[rgb(var(--success-6))]' />
            {completedLabel ?? actionLabel}
          </span>
        ) : item.skillHub ? (
          <span className='ml-auto shrink-0 text-t-tertiary'>v{item.skillHub.version}</span>
        ) : null}
        <span className={isCompleted || item.skillHub ? undefined : 'ml-auto'}>
          <Dropdown
            trigger='click'
            droplist={
              <Menu>
                <Menu.Item key='open-source' onClick={() => onOpenSource(item)}>
                  <LinkOne size={14} fill='currentColor' /> {t('settings.market.openSource', { defaultValue: '打开来源' })}
                </Menu.Item>
                {showInstallCommand && (
                  <Menu.Item key='copy-command' onClick={() => onCopyInstallCommand(item)}>
                    {t('settings.market.copyInstallCommand', { defaultValue: '复制安装命令' })}
                  </Menu.Item>
                )}
              </Menu>
            }
          >
            <Button
              ref={moreButtonRef}
              size='mini'
              type='text'
              aria-label={t('common.more', { defaultValue: '更多操作' })}
              className='!h-32px !w-32px !rounded-8px !p-0 !text-t-secondary hover:!bg-fill-1 hover:!text-t-primary active:!scale-96 motion-reduce:active:!transform-none'
              icon={<More theme='outline' size={16} fill='currentColor' />}
            />
          </Dropdown>
        </span>
      </header>

      <div className='mt-16px grid grid-cols-[auto_minmax(0,1fr)] items-start gap-12px'>
        <span
          className='flex h-40px w-40px min-w-40px items-center justify-center overflow-hidden rounded-10px bg-[var(--color-fill-2)] text-12px font-semibold text-t-secondary shadow-[inset_0_1px_0_color-mix(in_srgb,white_70%,transparent)]'
          aria-label={item.rank ? `${t('settings.market.rank', { defaultValue: '排名' })} ${item.rank}` : undefined}
        >
          {avatarSrc ? (
            <img
              src={avatarSrc}
              alt=''
              width={40}
              height={40}
              referrerPolicy='no-referrer'
              className='h-40px w-40px object-contain'
              onError={() => setBrokenAvatar(item.avatar)}
            />
          ) : item.rank ? (
            `#${item.rank}`
          ) : (
            '–'
          )}
        </span>
        <h3
          className='m-0 overflow-hidden text-16px font-medium leading-22px tracking-[-0.02em] text-t-primary'
          title={item.title}
          style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
        >
          {item.title}
        </h3>
      </div>

      {item.requiresApi && (
        <div className='mt-8px'>
          <Tag size='small' bordered={false} className='!rounded-6px !bg-fill-2 !text-10px !text-t-secondary'>
            {t('settings.skillsMarket.requiresApi', { defaultValue: '需要 API Key' })}
          </Tag>
        </div>
      )}

      <p
        className='mb-0 mt-10px min-h-40px overflow-hidden text-13px leading-20px text-t-secondary'
        title={item.summary || undefined}
        style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
      >
        {item.summary || t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' })}
      </p>

      {item.skillHub ? (
        <div className='mt-10px flex min-w-0 flex-wrap items-center gap-x-8px gap-y-4px text-12px text-t-tertiary'>
          {item.skillHub.category && <span className='max-w-full truncate'>{item.skillHub.category}</span>}
          <span title={item.skillHub.downloads.toLocaleString(i18n.language)}>
            {formatSkillHubMarketCount(item.skillHub.downloads)} {t('settings.skillsMarket.downloadUnit', { defaultValue: '下载' })}
          </span>
          <span>{t('settings.skillsMarket.score', { score: item.skillHub.score.toFixed(1), defaultValue: '热度分 {{score}}' })}</span>
        </div>
      ) : item.compactStats ? (
        <div className='mt-10px truncate text-12px text-t-tertiary'>{item.compactStats}</div>
      ) : null}

      {item.visibleTags.length > 0 && (
        <div className='mt-12px flex flex-wrap items-center gap-8px'>
          {item.visibleTags.slice(0, MAX_VISIBLE_TAGS).map((tag) => (
            <span
              key={tag}
              className='inline-flex max-w-[156px] items-center truncate text-12px leading-18px text-t-tertiary'
              title={tag}
            >
              {tag}
            </span>
          ))}
          {item.overflowTagCount > 0 && <span className='text-12px text-t-tertiary'>+{item.overflowTagCount}</span>}
        </div>
      )}

      <footer className='mt-auto pt-10px'>
        <div className='flex items-center justify-between gap-8px'>
          <Button
            type='text'
            size='mini'
            className='!h-32px !self-start !rounded-8px !px-0 !text-13px !text-t-tertiary hover:!text-t-primary'
            onClick={(event) => onViewDetails(item, event.currentTarget as HTMLElement)}
          >
            {t('settings.market.viewDetails', { defaultValue: '查看详情' })}
          </Button>
          <Button
            size='mini'
            type={isCompleted ? 'outline' : 'primary'}
            data-testid={`btn-add-market-skill-${testId}`}
            loading={busy || isChecking}
            disabled={disabled}
            className='flowy-icon-text-btn !h-32px !min-w-88px !rounded-12px !px-12px !text-12px !whitespace-nowrap active:!scale-96 motion-reduce:active:!transform-none'
            icon={
              isCompleted ? (
                <Check theme='outline' size={12} strokeWidth={3} fill='currentColor' />
              ) : !busy && !isChecking ? (
                <Plus theme='outline' size={12} strokeWidth={3} fill='currentColor' />
              ) : undefined
            }
            onClick={() => onAdd(item)}
          >
            {actionText}
          </Button>
        </div>
      </footer>
    </MarketCardShell>
  );
};

export default SkillMarketCard;
