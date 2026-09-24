import { ShoppingCart } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
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
        'inline-flex items-center justify-center rd-5px transition-all duration-150',
        'cursor-pointer text-[var(--warning-6,#f59e0b)] hover:text-[#fbbf24] hover:bg-[rgba(245,158,11,0.18)] active:scale-95',
        'focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_rgba(245,158,11,0.4)]',
        size === 'xs' ? 'size-18px p-1px' : 'size-22px p-2px',
        className
      )}
    >
      <ShoppingCart
        theme='filled'
        size={size === 'xs' ? '13' : '15'}
        fill='currentColor'
        className='block leading-none'
      />
    </span>
  );
};

export default CreditsWebsiteButton;
