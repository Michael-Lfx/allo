import { Button, Dropdown, Menu, Tag } from '@arco-design/web-react';
import { Check, LinkOne, More, Plus } from '@icon-park/react';
import React, { useEffect, useRef, useState } from 'react';
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
  const [avatarBroken, setAvatarBroken] = useState(false);

  useEffect(() => {
    setAvatarBroken(false);
  }, [item.avatar]);

  const avatarSrc = item.avatar && !avatarBroken ? item.avatar : undefined;
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
    <MarketCardShell testId={`skill-market-card-${testId}`}>
      <header className='grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-12px'>
        <span
          className='flex h-40px w-40px min-w-40px items-center justify-center overflow-hidden rounded-10px bg-[var(--color-fill-2)] text-12px font-semibold text-t-secondary'
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
              onError={() => setAvatarBroken(true)}
            />
          ) : item.rank ? (
            `#${item.rank}`
          ) : (
            '–'
          )}
        </span>
        <div className='min-w-0 pt-2px'>
          <h3
            className='m-0 overflow-hidden text-15px font-semibold leading-22px text-t-primary'
            title={item.title}
            style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
          >
            {item.title}
          </h3>
          {item.skillHub && (
            <div className='mt-3px truncate text-11px text-t-tertiary'>v{item.skillHub.version}</div>
          )}
          <div className='mt-4px'>
            <Tag size='small' bordered={false} className='!rounded-6px !bg-fill-2 !text-t-secondary !text-10px'>
              {item.marketSource === 'clawhub'
                ? 'ClawHub'
                : item.marketSource === 'skillhub'
                  ? 'SkillHub'
                  : item.upstreamSource || t('settings.skillsMarket.sourceUnknown', { defaultValue: '来源未知' })}
            </Tag>
          </div>
          {(item.requiresApi || item.noApi || item.apiKeyUnknown) && (
            <div className='mt-4px flex flex-wrap items-center gap-4px'>
              <Tag size='small' bordered={false} className='!rounded-6px !bg-fill-2 !text-t-secondary !text-10px'>
                {item.requiresApi
                  ? t('settings.skillsMarket.requiresApi', { defaultValue: '需要 API Key' })
                  : item.noApi
                    ? t('settings.skillsMarket.noApi', { defaultValue: '无需 API Key' })
                    : t('settings.skillsMarket.apiKeyUnknown', { defaultValue: 'API Key 状态未知' })}
              </Tag>
            </div>
          )}
        </div>
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
      </header>

      {item.compactStats && <div className='mt-8px truncate text-12px text-t-tertiary'>{item.compactStats}</div>}

      {item.skillHub && (
        <div className='mt-8px flex min-w-0 flex-wrap gap-6px text-11px text-t-tertiary'>
          {item.skillHub.category && <span className='truncate'>{item.skillHub.category}</span>}
          {item.skillHub.subCategories.slice(0, 2).map((category) => (
            <span key={category.key} className='truncate'>{category.name}</span>
          ))}
        </div>
      )}

      {item.skillHub && (
        <div className='mt-8px flex flex-wrap gap-x-10px gap-y-4px text-11px text-t-tertiary'>
          <span title={item.skillHub.downloads.toLocaleString(i18n.language)}>{formatSkillHubMarketCount(item.skillHub.downloads)} {t('settings.skillsMarket.downloadUnit', { defaultValue: '下载' })}</span>
          <span>{t('settings.skillsMarket.installs', { count: item.skillHub.installs, defaultValue: '{{count}} 安装' })}</span>
          <span>{t('settings.skillsMarket.stars', { count: item.skillHub.stars, defaultValue: '{{count}} 收藏' })}</span>
          <span>{t('settings.skillsMarket.score', { score: item.skillHub.score.toFixed(1), defaultValue: '热度分 {{score}}' })}</span>
          {item.skillHub.updatedAt && (
            <span>
              {t('settings.skillsMarket.updatedAt', {
                time: new Date(item.skillHub.updatedAt).toLocaleDateString(i18n.language),
                defaultValue: '更新 {{time}}',
              })}
            </span>
          )}
        </div>
      )}

      <p
        className='mb-0 mt-10px overflow-hidden text-13px leading-20px text-t-secondary'
        title={item.summary || undefined}
        style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
      >
        {item.summary || t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' })}
      </p>

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
