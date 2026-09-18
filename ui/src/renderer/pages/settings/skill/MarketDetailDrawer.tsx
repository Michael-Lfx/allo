import { Button, Drawer, Tag } from '@arco-design/web-react';
import { LinkOne } from '@icon-park/react';
import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { MarketActionState, MarketPrimaryActionConfig } from './marketContracts';
import { marketSourceLabel } from './skillMarket';
import { formatSkillHubMarketCount } from './marketViewModel';
import type { MarketItemViewModel } from './marketViewModel';

type MarketDetailDrawerProps = {
  item: MarketItemViewModel | null;
  visible: boolean;
  action: MarketPrimaryActionConfig;
  actionState?: MarketActionState;
  busy?: boolean;
  disabled?: boolean;
  showInstallCommand?: boolean;
  onPrimaryAction: (item: MarketItemViewModel) => void;
  onCopyInstallCommand: (item: MarketItemViewModel) => void;
  onOpenSource: (item: MarketItemViewModel) => void;
  onClose: () => void;
  restoreFocusRef?: React.MutableRefObject<HTMLElement | null>;
};

const MarketDetailDrawer: React.FC<MarketDetailDrawerProps> = ({
  item,
  visible,
  action,
  actionState = 'ready',
  busy = false,
  disabled = false,
  showInstallCommand = true,
  onPrimaryAction,
  onCopyInstallCommand,
  onOpenSource,
  onClose,
  restoreFocusRef,
}) => {
  const { t } = useTranslation();
  const headingRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => headingRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [visible, item?.id]);

  useEffect(() => {
    if (visible || !restoreFocusRef?.current) return;
    restoreFocusRef.current.focus();
    restoreFocusRef.current = null;
  }, [restoreFocusRef, visible]);

  return (
    <Drawer
      visible={visible && item !== null}
      width={Math.min(640, typeof window === 'undefined' ? 640 : window.innerWidth)}
      placement='right'
      title={
        <div ref={headingRef} tabIndex={-1} className='min-w-0 outline-none'>
          <div className='flex min-w-0 items-center gap-10px'>
            {item?.avatar ? (
              <img
                src={item.avatar}
                alt=''
                width={32}
                height={32}
                referrerPolicy='no-referrer'
                className='h-32px w-32px shrink-0 rounded-8px object-contain'
              />
            ) : null}
            <div className='min-w-0'>
              <div className='truncate text-16px font-semibold text-t-primary'>{item?.title}</div>
              <div className='mt-2px text-12px font-normal text-t-tertiary'>{t('settings.market.details', { defaultValue: '市场详情' })}</div>
            </div>
          </div>
        </div>
      }
      onCancel={onClose}
      footer={
        item ? (
          <div className='flex flex-wrap items-center justify-end gap-8px'>
            {showInstallCommand && (
              <Button className='!h-36px !rounded-8px' onClick={() => onCopyInstallCommand(item)}>
                {t('settings.market.copyInstallCommand', { defaultValue: '复制安装命令' })}
              </Button>
            )}
            <Button className='flowy-icon-text-btn !h-36px !rounded-8px' onClick={() => onOpenSource(item)} icon={<LinkOne size={14} fill='currentColor' />}>
              {t('settings.market.openSource', { defaultValue: '打开来源' })}
            </Button>
            <Button
              type='primary'
              className='!h-36px !min-w-100px !rounded-8px !whitespace-nowrap'
              loading={busy || actionState === 'checking'}
              disabled={disabled}
              onClick={() => onPrimaryAction(item)}
            >
              {actionState === 'checking'
                ? t('common.loading', { defaultValue: '加载中' })
                : actionState === 'completed'
                  ? (action.completedLabel ?? action.label)
                  : busy
                    ? action.pendingLabel
                    : action.label}
            </Button>
          </div>
        ) : null
      }
    >
      {item && (
        <div className='space-y-20px overflow-y-auto pb-8px'>
          <p className='m-0 text-14px leading-22px text-t-secondary'>{item.fullDescription}</p>
          <div className='grid grid-cols-2 gap-16px text-12px'>
            <div>
              <div className='text-t-tertiary'>{t('settings.market.source', { defaultValue: '来源' })}</div>
              <div className='mt-4px text-t-primary'>
                {item.marketSource === 'clawhub'
                  ? 'ClawHub'
                  : item.marketSource === 'skillhub'
                    ? 'SkillHub'
                    : item.upstreamSource || marketSourceLabel(item.source)}
              </div>
            </div>
            {item.upstreamSource && (
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.upstreamSource', { defaultValue: '上游来源' })}</div>
                <div className='mt-4px break-all text-t-primary'>{item.upstreamSource}</div>
              </div>
            )}
            {item.fullStats && (
              <div>
                <div className='text-t-tertiary'>{t('settings.market.statistics', { defaultValue: '统计' })}</div>
                <div className='mt-4px text-t-primary'>{item.fullStats}</div>
              </div>
            )}
          </div>
          {item.skillHub && (
            <div className='grid grid-cols-2 gap-16px text-12px'>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.version', { defaultValue: '版本' })}</div>
                <div className='mt-4px text-t-primary'>{item.skillHub.version}</div>
              </div>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.owner', { defaultValue: '作者' })}</div>
                <div className='mt-4px break-all text-t-primary'>{item.skillHub.owner}</div>
              </div>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.slug', { defaultValue: 'Slug' })}</div>
                <div className='mt-4px break-all text-t-primary'>{item.skillHub.slug}</div>
              </div>
              {item.skillHub.category && (
                <div>
                  <div className='text-t-tertiary'>{t('settings.skillsMarket.category', { defaultValue: '分类' })}</div>
                  <div className='mt-4px text-t-primary'>{item.skillHub.category}</div>
                </div>
              )}
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.downloadsLabel', { defaultValue: '下载量' })}</div>
                <div className='mt-4px text-t-primary' title={item.skillHub.downloads.toLocaleString()}>{formatSkillHubMarketCount(item.skillHub.downloads)}</div>
              </div>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.installsLabel', { defaultValue: '安装量' })}</div>
                <div className='mt-4px text-t-primary'>{item.skillHub.installs}</div>
              </div>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.starsLabel', { defaultValue: '收藏量' })}</div>
                <div className='mt-4px text-t-primary'>{item.skillHub.stars}</div>
              </div>
              <div>
                <div className='text-t-tertiary'>{t('settings.skillsMarket.scoreLabel', { defaultValue: '热度分' })}</div>
                <div className='mt-4px text-t-primary'>{item.skillHub.score.toFixed(1)}</div>
              </div>
              {item.skillHub.updatedAt && (
                <div>
                  <div className='text-t-tertiary'>{t('settings.skillsMarket.updatedAtLabel', { defaultValue: '更新时间' })}</div>
                  <div className='mt-4px text-t-primary'>{new Date(item.skillHub.updatedAt).toLocaleString()}</div>
                </div>
              )}
              {item.skillHub.createdAt && (
                <div>
                  <div className='text-t-tertiary'>{t('settings.skillsMarket.createdAtLabel', { defaultValue: '创建时间' })}</div>
                  <div className='mt-4px text-t-primary'>{new Date(item.skillHub.createdAt).toLocaleString()}</div>
                </div>
              )}
            </div>
          )}
          {(item.requiresApi || item.noApi || item.apiKeyUnknown) && (
            <div className='flex flex-wrap gap-6px'>
              <Tag bordered={false} className='!bg-fill-2 !text-t-secondary'>
                {item.requiresApi
                  ? t('settings.skillsMarket.requiresApi', { defaultValue: '需要 API Key' })
                  : item.noApi
                    ? t('settings.skillsMarket.noApi', { defaultValue: '无需 API Key' })
                    : t('settings.skillsMarket.apiKeyUnknown', { defaultValue: 'API Key 状态未知' })}
              </Tag>
            </div>
          )}
          {item.allTags.length > 0 && (
            <div>
              <div className='mb-8px text-12px text-t-tertiary'>{t('settings.presetTags', { defaultValue: '标签' })}</div>
              <div className='flex flex-wrap gap-6px'>
                {item.allTags.map((tag) => (
                  <Tag key={tag} size='small' bordered={false} className='!bg-fill-2 !text-t-secondary'>
                    {tag}
                  </Tag>
                ))}
              </div>
            </div>
          )}
          {item.skillHub && item.skillHub.subCategories.length > 0 && (
            <div>
              <div className='mb-8px text-12px text-t-tertiary'>{t('settings.skillsMarket.subCategories', { defaultValue: '子分类' })}</div>
              <div className='flex flex-wrap gap-6px'>
                {item.skillHub.subCategories.map((category) => (
                  <Tag key={category.key} size='small' bordered={false} className='!bg-fill-2 !text-t-secondary'>
                    {category.name}
                  </Tag>
                ))}
              </div>
            </div>
          )}
          {showInstallCommand && (
            <div>
              <div className='mb-8px text-12px text-t-tertiary'>{t('settings.market.installCommand', { defaultValue: '安装命令' })}</div>
              <code className='block break-all rounded-8px bg-fill-2 p-12px text-12px leading-18px text-t-primary'>
                {item.installCommand}
              </code>
            </div>
          )}
        </div>
      )}
    </Drawer>
  );
};

export default MarketDetailDrawer;
