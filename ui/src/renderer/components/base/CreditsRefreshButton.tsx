import { Refresh } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';

import { useCredits } from '@/renderer/hooks/context/CreditsContext';

type CreditsRefreshButtonProps = {
  /** xs = 侧栏弹层（12px 图标）；sm = 设置页头（14px 图标）。 */
  size?: 'xs' | 'sm';
  /** 额外类名（位置 / 外边距）。 */
  className?: string;
};

// 皮肤强调色：--primary-6 是每套皮肤都定义的 Arco 主色 RGB 三元组，换肤即变色。
// 刻意不用 --flowy-accent（静态青，6 套皮肤无一覆盖）——那是积分元素原本「不契合主题」的根因。
// 节流态用强调色 (n)s 括号注解表达：括号是结构分隔（不会被误读成「1」），不套软洗方框（多余立体 + 错位）。
const ACCENT = 'rgb(var(--primary-6))';

/**
 * 积分余额刷新按钮 —— 侧栏头像弹层与媒体设置页头共用。
 * 三态：空闲（中性）/ 加载中（强调色旋转图标）/ 节流倒计时（强调色 (n)s 括号注解；弃用竖线，因其紧贴数字会被误读成「1」）。
 * 强调色全部经 --primary-6 跟随当前皮肤，切肤即变色；不引用任何静态 token。
 *
 * 根元素用 span[role=button] 而非原生 button 元素：原生 button 自带 UA 内边距 / 最小高 / OS
 * 外观，会令倒计时方框高出余额数字所在的文本行、产生上下错位。span 无这些 UA 负担，高度随内容
 * （图标 / 数字），在外层 flex items-center 下与相邻余额数字天然等高对齐。
 */
const CreditsRefreshButton: React.FC<CreditsRefreshButtonProps> = ({ size = 'sm', className }) => {
  const { t } = useTranslation();
  const { isFetchingBalance, cooldownSeconds, canRefresh, manualRefresh } = useCredits();

  const label = t('common.userMenu.refreshCredits', { defaultValue: '刷新积分余额' });
  const inCooldown = cooldownSeconds > 0;
  // 主题态（加载 / 倒计时）靠自身配色表达；仅空闲态沿用中性 hover。
  const themedState = inCooldown || isFetchingBalance;

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (!canRefresh) return;
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      manualRefresh();
    }
  };

  return (
    <span
      role='button'
      tabIndex={canRefresh ? 0 : -1}
      aria-label={label}
      aria-disabled={!canRefresh}
      onClick={canRefresh ? manualRefresh : undefined}
      onKeyDown={handleKeyDown}
      className={classNames(
        // 固定宽度：图标态与「(n)s 括号注解」态同宽，点刷新切换不触发宽度重绘。
        'inline-flex items-center justify-center bg-transparent rd-4px transition-colors',
        canRefresh ? 'cursor-pointer' : 'cursor-not-allowed',
        // 主题焦点环：与 control contract 对齐（--control-focus-ring 每套皮肤都定义）。
        'focus-visible:outline-none focus-visible:shadow-[0_0_0_3px_var(--control-focus-ring)]',
        // 主题态不着中性色；仅空闲可点时挂 hover，未登录（不可点）时半透明。
        !themedState && canRefresh && 'text-t-tertiary hover:bg-fill-2 hover:text-t-secondary',
        !themedState && !canRefresh && 'text-t-tertiary opacity-50',
        // 槽位按 (n)s 倒计时定宽：xs/sm 倒计时同为 12px，(n)s ≈21px，w-28px 容纳有余、图标态同宽。
        'w-28px',
        className
      )}
    >
      {inCooldown ? (
        // 倒计时用括号注解 (n)s：括号是结构分隔，不会像竖线那样紧贴数字被误读成「1」。
        // tabular-nums 让 n=1..5 字符等宽，(n)s 整体宽度恒定，配合外层固定槽位无重绘。
        <span className='tabular-nums text-12px leading-none' style={{ color: ACCENT }}>
          ({cooldownSeconds}s)
        </span>
      ) : (
        <Refresh
          theme='outline'
          size={size === 'xs' ? '12' : '14'}
          fill='currentColor'
          // block + leading-none：压掉 @icon-park 图标默认行高，避免图标盒高于余额数字、视觉上浮。
          className={classNames('block leading-none', isFetchingBalance && 'motion-safe:animate-spin')}
          style={isFetchingBalance ? { color: ACCENT } : undefined}
        />
      )}
    </span>
  );
};

export default CreditsRefreshButton;
