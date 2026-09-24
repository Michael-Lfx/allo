import { ShoppingCart } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { openOfficialWebsiteCredits } from '@renderer/utils/openOfficialWebsiteCredits';

type CreditsWebsiteButtonProps = {
  /** xs = 侧栏底部（18px 紧凑图标）；sm = 弹层菜单（充值药丸/胶囊）；md = 设置页 */
  size?: 'xs' | 'sm';
  variant?: 'icon' | 'pill';
  className?: string;
};

/**
  * 积分余额旁的充值/购买按钮：打开官网积分增值 tab（带云 JWT 自动登录）。
  */
const CreditsWebsiteButton: React.FC<CreditsWebsiteButtonProps> = ({
  size = 'sm',
  variant = 'icon',
  className,
}) => {
  const { t } = useTranslation();
  const { balance } = useCredits();
  const label = t('billing.openBilling');

  const openBilling = () => {
    void openOfficialWebsiteCredits(undefined, undefined, { source: 'sider', balance });
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      openBilling();
    }
  };

  if (variant === 'pill') {
    return (
      <span
        role='button'
        tabIndex={0}
        aria-label={label}
        title={label}
        onClick={(event) => {
          event.stopPropagation();
          openBilling();
        }}
        onKeyDown={handleKeyDown}
        className={classNames(
          'credits-topup-btn inline-flex items-center gap-4px px-8px py-2px rd-full text-11px font-600 cursor-pointer select-none leading-none',
          'focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_rgba(245,158,11,0.4)]',
          className
        )}
      >
        <ShoppingCart
          theme='outline'
          size='12'
          strokeWidth={3.5}
          fill='currentColor'
          className='block leading-none shrink-0'
        />
        <span className='leading-none'>{t('common.creditsBubble.topUpAction', { defaultValue: '充值' })}</span>
      </span>
    );
  }

  return (
    <span
      role='button'
      tabIndex={0}
      aria-label={label}
      title={label}
      onClick={(event) => {
        event.stopPropagation();
        openBilling();
      }}
      onKeyDown={handleKeyDown}
      className={classNames(
        'credits-topup-btn inline-flex items-center justify-center rd-5px',
        'cursor-pointer focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_rgba(245,158,11,0.4)]',
        size === 'xs' ? 'size-18px p-1px' : 'size-22px p-2px',
        className
      )}
    >
      <ShoppingCart
        theme='outline'
        size={size === 'xs' ? '12' : '14'}
        strokeWidth={3.5}
        fill='currentColor'
        className='block leading-none'
      />
    </span>
  );
};

export default CreditsWebsiteButton;

