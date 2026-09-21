import { ShoppingCart } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { openOfficialWebsiteCredits } from '@renderer/utils/openOfficialWebsiteCredits';

type CreditsWebsiteButtonProps = {
  /** xs = 侧栏弹层（12px 图标）；sm = 设置页头（14px 图标）。 */
  size?: 'xs' | 'sm';
  className?: string;
};

/**
 * 积分余额旁的购物车按钮：打开官网积分增值 tab（带云 JWT 自动登录）。
 */
const CreditsWebsiteButton: React.FC<CreditsWebsiteButtonProps> = ({ size = 'sm', className }) => {
  const { t } = useTranslation();
  const label = t('billing.openBilling');

  const openBilling = () => {
    void openOfficialWebsiteCredits();
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      openBilling();
    }
  };

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
        'inline-flex items-center justify-center bg-transparent rd-4px transition-colors',
        'cursor-pointer text-t-tertiary hover:bg-fill-2 hover:text-t-secondary active:bg-fill-3',
        'focus-visible:outline-none focus-visible:shadow-[0_0_0_3px_var(--control-focus-ring)]',
        size === 'xs' ? 'size-18px' : 'size-22px',
        className
      )}
    >
      <ShoppingCart
        theme='outline'
        size={size === 'xs' ? '12' : '14'}
        fill='currentColor'
        className='block leading-none'
      />
    </span>
  );
};

export default CreditsWebsiteButton;
