import { ShoppingCart } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import { resolveCreditsBubbleState } from '@/renderer/utils/credits/creditsBubbleModel';
import { openOfficialWebsiteCredits } from '@renderer/utils/openOfficialWebsiteCredits';

type CreditsWebsiteButtonProps = {
  /** xs = 侧栏底部（14px 紧凑内联图标）；sm = 18px 独立方块；md = 22px 设置页 */
  size?: 'xs' | 'sm';
  variant?: 'icon' | 'pill';
  className?: string;
};

/**
  * 积分余额旁的充值/购买按钮：打开官网积分增值 tab（带云 JWT 自动登录）。
  * 视觉层级根据当前积分状态（正常、低额度预警、已耗尽）动态响应。
  */
const CreditsWebsiteButton: React.FC<CreditsWebsiteButtonProps> = ({
  size = 'sm',
  variant = 'icon',
  className,
}) => {
  const { t } = useTranslation();
  const credits = useCredits();
  const { balance } = credits;
  const label = t('billing.openBilling');

  const bubbleState = resolveCreditsBubbleState(credits);

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
          'credits-topup-btn inline-flex items-center gap-2.5px px-5px h-18px rd-full text-10.5px font-500 cursor-pointer select-none leading-none',
          `credits-topup-btn--${bubbleState}`,
          'focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_rgba(245,158,11,0.4)]',
          className
        )}
      >
        <ShoppingCart
          theme='outline'
          size='10'
          strokeWidth={2.8}
          fill='currentColor'
          className='block leading-none shrink-0'
        />
        <span className='leading-none tracking-tight'>{t('common.creditsBubble.topUpAction', { defaultValue: '充值' })}</span>
      </span>
    );
  }

  const isXs = size === 'xs';

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
        'inline-flex items-center justify-center transition-all duration-150',
        isXs
          ? classNames('credits-topup-icon-btn size-14px rd-3px', `credits-topup-icon-btn--${bubbleState}`)
          : classNames('credits-topup-btn size-18px p-1px rd-5px', `credits-topup-btn--${bubbleState}`),
        'cursor-pointer focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_rgba(245,158,11,0.4)]',
        className
      )}
    >
      <ShoppingCart
        theme='outline'
        size={isXs ? '10.5' : '13'}
        strokeWidth={isXs ? 3 : 3.5}
        fill='currentColor'
        className='block leading-none'
      />
    </span>
  );
};

export default CreditsWebsiteButton;


