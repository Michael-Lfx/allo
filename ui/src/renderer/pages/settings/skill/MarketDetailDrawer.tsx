import { Button, Drawer, Tag } from '@arco-design/web-react';
import { LinkOne } from '@icon-park/react';
import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { MarketActionState, MarketPrimaryActionConfig } from './marketContracts';
import { marketSourceLabel } from './skillMarket';
import type { MarketItemViewModel } from './marketViewModel';

type MarketDetailDrawerProps = {
  item: MarketItemViewModel | null;
  visible: boolean;
  action: MarketPrimaryActionConfig;
  actionState?: MarketActionState;
  busy?: boolean;
  disabled?: boolean;
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
          <div className='truncate text-16px font-semibold text-t-primary'>{item?.title}</div>
          <div className='mt-2px text-12px font-normal text-t-tertiary'>{t('settings.market.details', { defaultValue: '市场详情' })}</div>
        </div>
      }
      onCancel={onClose}
      footer={
        item ? (
          <div className='flex flex-wrap items-center justify-end gap-8px'>
            <Button className='!h-36px !rounded-8px' onClick={() => onCopyInstallCommand(item)}>
              {t('settings.market.copyInstallCommand', { defaultValue: '复制安装命令' })}
            </Button>
            <Button className='!h-36px !rounded-8px' onClick={() => onOpenSource(item)} icon={<LinkOne size={14} fill='currentColor' />}>
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
              <div className='mt-4px text-t-primary'>{marketSourceLabel(item.source)}</div>
            </div>
            {item.fullStats && (
              <div>
                <div className='text-t-tertiary'>{t('settings.market.statistics', { defaultValue: '统计' })}</div>
                <div className='mt-4px text-t-primary'>{item.fullStats}</div>
              </div>
            )}
          </div>
          {(item.requiresApi || item.noApi) && (
            <div className='flex flex-wrap gap-6px'>
              <Tag bordered={false} className='!bg-fill-2 !text-t-secondary'>
                {item.requiresApi
                  ? t('settings.market.requiresApi', { defaultValue: '需 API' })
                  : t('settings.market.noApi', { defaultValue: '免 API' })}
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
          <div>
            <div className='mb-8px text-12px text-t-tertiary'>{t('settings.market.installCommand', { defaultValue: '安装命令' })}</div>
            <code className='block break-all rounded-8px bg-fill-2 p-12px text-12px leading-18px text-t-primary'>
              {item.installCommand}
            </code>
          </div>
        </div>
      )}
    </Drawer>
  );
};

export default MarketDetailDrawer;
