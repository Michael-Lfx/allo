import { Button, Dropdown, Menu, Tag } from '@arco-design/web-react';
import { Check, LinkOne, More, Plus } from '@icon-park/react';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { normalizeTestId } from './skillPresentation';
import MarketCardShell from './MarketCardShell';
import type { MarketActionState } from './marketContracts';
import type { MarketItemViewModel } from './marketViewModel';

type SkillMarketCardProps = {
  item: MarketItemViewModel;
  actionLabel: string;
  pendingLabel: string;
  completedLabel?: string;
  actionState?: MarketActionState;
  busy?: boolean;
  disabled?: boolean;
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
  onAdd,
  onOpenSource,
  onCopyInstallCommand,
  onViewDetails,
}) => {
  const { t } = useTranslation();
  const testId = normalizeTestId(item.id);
  const moreButtonRef = useRef<HTMLButtonElement>(null);
  const [avatarBroken, setAvatarBroken] = useState(false);

  useEffect(() => {
    setAvatarBroken(false);
  }, [item.avatar]);

  const avatarSrc = item.avatar && !avatarBroken ? item.avatar : undefined;

  return (
    <MarketCardShell testId={`skill-market-card-${testId}`}>
      <header className='grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-10px'>
        <span
          className='flex h-32px w-32px min-w-32px items-center justify-center overflow-hidden rounded-8px bg-[var(--color-fill-2)] text-12px font-semibold text-t-secondary'
          aria-label={item.rank ? `${t('settings.market.rank', { defaultValue: '排名' })} ${item.rank}` : undefined}
        >
          {avatarSrc ? (
            <img
              src={avatarSrc}
              alt=''
              width={32}
              height={32}
              referrerPolicy='no-referrer'
              className='h-32px w-32px object-contain'
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
            className='m-0 overflow-hidden text-14px font-semibold leading-20px text-t-primary'
            title={item.title}
            style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
          >
            {item.title}
          </h3>
          {(item.requiresApi || item.noApi) && (
            <div className='mt-4px flex flex-wrap items-center gap-4px'>
              <Tag size='small' bordered={false} className='!rounded-6px !bg-fill-2 !text-t-secondary !text-10px'>
                {item.requiresApi
                  ? t('settings.market.requiresApi', { defaultValue: '需 API' })
                  : t('settings.market.noApi', { defaultValue: '免 API' })}
              </Tag>
            </div>
          )}
        </div>
        <div className='flex shrink-0 items-center gap-4px'>
          <Button
            size='mini'
            type='primary'
            data-testid={`btn-add-market-skill-${testId}`}
            loading={busy || actionState === 'checking'}
            disabled={disabled}
            className='flowy-icon-text-btn !h-32px !min-w-88px !rounded-8px !px-10px !text-12px !whitespace-nowrap active:!scale-96 motion-reduce:active:!transform-none'
            icon={
              actionState === 'completed' ? (
                <Check theme='outline' size={12} strokeWidth={3} fill='currentColor' />
              ) : !busy && actionState !== 'checking' ? (
                <Plus theme='outline' size={12} strokeWidth={3} fill='currentColor' />
              ) : undefined
            }
            onClick={() => onAdd(item)}
          >
            {actionState === 'checking' ? t('common.loading', { defaultValue: '加载中' }) : actionState === 'completed' ? (completedLabel ?? actionLabel) : busy ? pendingLabel : actionLabel}
          </Button>
          <Dropdown
            trigger='click'
            droplist={
              <Menu>
                <Menu.Item key='open-source' onClick={() => onOpenSource(item)}>
                  <LinkOne size={14} fill='currentColor' /> {t('settings.market.openSource', { defaultValue: '打开来源' })}
                </Menu.Item>
                <Menu.Item key='copy-command' onClick={() => onCopyInstallCommand(item)}>
                  {t('settings.market.copyInstallCommand', { defaultValue: '复制安装命令' })}
                </Menu.Item>
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
        </div>
      </header>

      {item.compactStats && <div className='mt-7px truncate text-11px text-t-tertiary'>{item.compactStats}</div>}

      <p
        className='mb-0 mt-10px overflow-hidden text-12px leading-18px text-t-secondary'
        title={item.summary || undefined}
        style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
      >
        {item.summary || t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' })}
      </p>

      {item.visibleTags.length > 0 && (
        <div className='mt-12px flex flex-wrap items-center gap-6px'>
          {item.visibleTags.slice(0, MAX_VISIBLE_TAGS).map((tag) => (
            <span
              key={tag}
              className='inline-flex max-w-[156px] items-center truncate rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-2)] px-8px py-1px text-11px leading-16px text-t-secondary'
              title={tag}
            >
              {tag}
            </span>
          ))}
          {item.overflowTagCount > 0 && <span className='text-11px text-t-tertiary'>+{item.overflowTagCount}</span>}
        </div>
      )}

      <footer className='mt-auto pt-10px'>
        <Button
          type='text'
          size='mini'
          className='!h-28px !self-start !rounded-6px !px-0 !text-12px !text-t-secondary hover:!text-t-primary'
          onClick={(event) => onViewDetails(item, event.currentTarget as HTMLElement)}
        >
          {t('settings.market.viewDetails', { defaultValue: '查看详情' })}
        </Button>
      </footer>
    </MarketCardShell>
  );
};

export default SkillMarketCard;
