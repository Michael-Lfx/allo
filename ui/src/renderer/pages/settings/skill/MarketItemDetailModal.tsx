import { Modal } from '@arco-design/web-react';
import { LinkOne, Lightning } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { ExpertMark } from '../PresetSettings/ExpertTile';
import type { MarketActionState, MarketPrimaryActionConfig } from './marketContracts';
import { formatSkillHubMarketCount } from './marketViewModel';
import type { MarketItemViewModel } from './marketViewModel';

type MarketItemDetailModalProps = {
  item: MarketItemViewModel | null;
  action: MarketPrimaryActionConfig;
  actionState: MarketActionState;
  busy: boolean;
  disabled: boolean;
  onPrimaryAction: (item: MarketItemViewModel) => void;
  onOpenSource: (item: MarketItemViewModel) => void;
  onClose: () => void;
};

const Divider: React.FC = () => <div className='h-1px bg-[var(--color-border-2)]' aria-hidden='true' />;

/** Market item detail with the same layout as the expert detail modal. */
const MarketItemDetailModal: React.FC<MarketItemDetailModalProps> = ({
  item,
  action,
  actionState,
  busy,
  disabled,
  onPrimaryAction,
  onOpenSource,
  onClose,
}) => {
  const { t, i18n } = useTranslation();
  const downloads = item?.skillHub?.downloads;
  const category = item?.skillHub?.category;
  const tags = category ? [category, ...(item?.allTags ?? []).filter((tag) => tag !== category)] : (item?.allTags ?? []);
  const actionText =
    actionState === 'checking'
      ? t('common.loading', { defaultValue: '加载中' })
      : actionState === 'completed'
        ? (action.completedLabel ?? action.label)
        : busy
          ? action.pendingLabel
          : action.label;

  return (
    <Modal
      visible={item !== null}
      unmountOnExit
      onCancel={onClose}
      footer={null}
      autoFocus={false}
      style={{ width: 560, maxWidth: '92vw', borderRadius: 16 }}
      data-testid='market-item-detail-modal'
    >
      {item ? (
        <div className='flex flex-col gap-20px text-13px leading-20px text-t-primary'>
          <div className='flex items-start gap-16px pr-32px'>
            <ExpertMark name={item.title} avatarUrl={item.avatar} size={64} />
            <div className='flex min-w-0 flex-1 flex-col items-start gap-8px'>
              <h2 className='m-0 max-w-full truncate text-18px font-semibold leading-26px'>{item.title}</h2>
              <button
                type='button'
                data-testid='market-item-detail-action'
                disabled={disabled || actionState === 'completed' || actionState === 'checking'}
                aria-busy={busy}
                className='cursor-pointer rounded-full border-none bg-[var(--color-text-1)] px-14px py-4px text-13px font-semibold leading-20px text-[var(--color-bg-1)] disabled:cursor-default disabled:opacity-60'
                onClick={() => onPrimaryAction(item)}
              >
                {actionText}
              </button>
            </div>
          </div>
          {item.summary ? <p className='m-0'>{item.summary}</p> : null}
          {tags.length > 0 ? (
            <div className='flex flex-wrap gap-6px'>
              {tags.map((tag) => (
                <span key={tag} className='rounded-6px bg-fill-2 px-10px py-4px text-12px leading-18px text-t-secondary'>
                  {tag}
                </span>
              ))}
            </div>
          ) : null}
          <Divider />
          <div className='flex justify-around text-center'>
            {item.skillHub?.version ? (
              <div className='flex flex-col items-center gap-4px'>
                <span className='text-15px font-semibold leading-22px'>{item.skillHub.version}</span>
                <span className='text-12px leading-18px text-t-tertiary'>
                  {t('settings.skillsMarket.version', { defaultValue: '版本' })}
                </span>
              </div>
            ) : null}
            <div className='flex flex-col items-center gap-4px'>
              <span className='flex items-center gap-4px text-15px font-semibold leading-22px'>
                <Lightning theme='outline' size={14} className='relative top-px' />
                {downloads !== undefined
                  ? formatSkillHubMarketCount(downloads)
                  : (item.compactStats ?? '0')}
              </span>
              <span
                className='text-12px leading-18px text-t-tertiary'
                title={downloads?.toLocaleString(i18n.language)}
              >
                {t('settings.skillsMarket.downloadsLabel', { defaultValue: '下载量' })}
              </span>
            </div>
          </div>
          <Divider />
          <p className='m-0 whitespace-pre-wrap text-t-secondary'>{item.fullDescription}</p>
          <button
            type='button'
            className='inline-flex w-fit cursor-pointer items-center gap-6px border-none bg-transparent p-0 text-13px text-t-secondary hover:text-t-primary'
            onClick={() => onOpenSource(item)}
          >
            <LinkOne size={14} fill='currentColor' />
            {t('settings.market.openSource', { defaultValue: '打开来源' })}
          </button>
        </div>
      ) : null}
    </Modal>
  );
};

export default MarketItemDetailModal;
