

import { useThemeContext } from '@/renderer/hooks/context/ThemeContext';
import type { ThemePreference } from '@/renderer/hooks/system/useTheme';
import { FLOWY_EASE, prefersReducedMotion } from '@/renderer/utils/motion/flowyMotion';
import { IconDesktop, IconMoon, IconSun } from '@arco-design/web-react/icon';
import React, { useRef } from 'react';
import { useTranslation } from 'react-i18next';

/**
 * 主题切换器组件 / Theme switcher component
 *
 * 提供跟随系统 / 浅色 / 深色三种模式切换
 * Provides follow-system / light / dark switching
 */

// Track vertical inset; must match the track's p-6px so the sliding capsule's
// rounded corners stay concentric with the track's (aligned, not clipped).
const TRACK_INSET = 6;

const OPTIONS: Array<{ value: ThemePreference; Icon: typeof IconSun }> = [
  { value: 'system', Icon: IconDesktop },
  { value: 'light', Icon: IconSun },
  { value: 'dark', Icon: IconMoon },
];

export const ThemeSwitcher = () => {
  const { theme, themePreference, setThemePreference } = useThemeContext();
  const { t } = useTranslation();
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const labels: Record<ThemePreference, string> = {
    light: t('settings.lightMode'),
    dark: t('settings.darkMode'),
    system: t('settings.systemMode'),
  };

  const activeIndex = Math.max(
    0,
    OPTIONS.findIndex((option) => option.value === themePreference)
  );

  // Animate only the composited transform — never layout properties. Snap
  // instantly when the user prefers reduced motion.
  const capsuleTransition = prefersReducedMotion()
    ? 'none'
    : `transform 260ms cubic-bezier(${FLOWY_EASE.enter.join(',')})`;

  const select = (index: number) => {
    const option = OPTIONS[index];
    if (!option || option.value === themePreference) return;
    void setThemePreference(option.value);
  };

  // Roving radiogroup keyboard support: arrows/Home/End move the selection and
  // focus together, keeping the control fully operable without a pointer.
  const onKeyDown = (event: React.KeyboardEvent, index: number) => {
    let next = -1;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (index + 1) % OPTIONS.length;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = (index - 1 + OPTIONS.length) % OPTIONS.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = OPTIONS.length - 1;
    if (next < 0 || next === index) return;
    event.preventDefault();
    select(next);
    buttonRefs.current[next]?.focus();
  };

  return (
    <div
      className='relative grid grid-cols-3 box-border p-6px rd-full border border-solid border-[var(--color-border-2)] bg-1 w-full min-w-0'
      role='radiogroup'
      aria-label={t('settings.theme')}
    >
      {/* Sliding capsule. The track's box-border + an inset-derived width keep
          the capsule concentric with the track's corners on both ends, so the
          left/right rounding aligns instead of being clipped by the panel. */}
      <span
        aria-hidden='true'
        className='absolute rd-full'
        style={{
          top: TRACK_INSET,
          bottom: TRACK_INSET,
          left: TRACK_INSET,
          width: `calc((100% - ${TRACK_INSET * 2}px) / ${OPTIONS.length})`,
          transform: `translateX(calc(${activeIndex} * 100%))`,
          transition: capsuleTransition,
          backgroundColor: 'var(--color-fill-2)',
          boxShadow: theme === 'dark' ? '0 1px 4px rgba(0, 0, 0, 0.18)' : '0 2px 8px rgba(0, 0, 0, 0.08)',
        }}
      />
      {OPTIONS.map((option, index) => {
        const isActive = index === activeIndex;
        const Icon = option.Icon;
        return (
          <button
            key={option.value}
            ref={(el) => {
              buttonRefs.current[index] = el;
            }}
            type='button'
            role='radio'
            aria-checked={isActive}
            tabIndex={isActive ? 0 : -1}
            className='relative z-1 h-33px min-w-0 px-4px rd-full text-12px font-500 inline-flex items-center justify-center gap-5px border-none bg-transparent select-none transition-all duration-180 active:scale-95 focus-visible:shadow-[0_0_0_2px_var(--control-focus-ring,var(--color-primary-light-3))]'
            style={{
              color: isActive ? (theme === 'dark' ? 'var(--color-text-1)' : 'rgb(var(--primary-6))') : 'var(--color-text-2)',
              cursor: isActive ? 'default' : 'pointer',
              outline: 'none',
            }}
            onClick={() => select(index)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            <Icon
              style={{
                fontSize: 14,
                transform: isActive ? 'scale(1.05)' : 'scale(1)',
                transition: 'transform 200ms ease',
              }}
            />
            <span className='truncate'>{labels[option.value]}</span>
          </button>
        );
      })}
    </div>
  );
};
