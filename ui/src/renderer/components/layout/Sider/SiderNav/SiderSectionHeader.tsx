

import React from 'react';
import classNames from 'classnames';

interface SiderSectionHeaderProps {
  /** Already-translated section label (e.g. "工作"). */
  label: string;
  /** Stable heading id for semantic navigation regions. */
  id?: string;
  /** Icon-only rail mode: show a hairline rule instead of the text label. */
  collapsed: boolean;
  /** Compact variant used by the workspace section heading. */
  compact?: boolean;
  /** Additional layout or theme classes for a specific sidebar surface. */
  className?: string;
  /** Optional controls rendered on the right side of the section heading. */
  actions?: React.ReactNode;
  /**
   * Whether to draw the hairline rule in collapsed mode. Defaults to true.
   * Set false where an enclosing `border-t` already separates the region
   * (e.g. the bottom-pinned group), to avoid doubling the line.
   */
  collapsedRule?: boolean;
}

/**
 * SiderSectionHeader — the small-text group label that segments the primary
 * navigation rail and the workspace list.
 *
 * Mirrors the Settings sider group-header (`text-t-tertiary font-[500]`), sized
 * down to 12px to read as a quiet section divider. In the collapsed icon-only
 * rail there is no room for text, so it degrades to a hairline rule that keeps
 * the 工作 / 资源 / 自动化 groups visually distinct.
 */
const SiderSectionHeader: React.FC<SiderSectionHeaderProps> = ({
  label,
  id,
  collapsed,
  compact = false,
  className,
  actions,
  collapsedRule = true,
}) => {
  if (collapsed) {
    if (!collapsedRule) return null;
    return <div className='shrink-0 mt-6px mb-2px h-1px bg-[var(--color-border-2)] mx-6px' />;
  }

  return (
    <div
      id={id}
      role={id ? 'heading' : undefined}
      aria-level={id ? 2 : undefined}
      className={classNames(
        compact
          ? 'shrink-0 h-32px pl-7px pr-4px flex items-center gap-8px text-12px font-[500] leading-none text-t-tertiary select-none'
          : 'shrink-0 mt-8px mb-2px px-12px h-22px flex items-center text-12px font-[500] leading-none text-t-tertiary select-none',
        className
      )}
    >
      <span className='min-w-0 truncate'>{label}</span>
      {actions && <span className='ml-auto shrink-0'>{actions}</span>}
    </div>
  );
};

export default SiderSectionHeader;
