import type { KeyboardEvent } from 'react';

/**
 * Keyboard / assistive-technology contract for sidebar navigation rows that are
 * rendered as `<div>` so their layout and the shared selection indicator
 * (`data-sider-nav-entry`) stay untouched.
 *
 * Adds `role="button"`, focusability, and Enter / Space activation. Pass `label`
 * for icon-only (collapsed) rows whose visible name lives only in a tooltip.
 * Spread it BEFORE any explicit attributes it should not override.
 */
export function navRowProps(onClick: (() => void) | undefined, label?: string) {
  return {
    role: 'button' as const,
    tabIndex: 0,
    'aria-label': label,
    onKeyDown: (event: KeyboardEvent<HTMLElement>) => {
      // Ignore keys bubbling up from nested interactive children.
      if (event.target !== event.currentTarget) return;
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        onClick?.();
      }
    },
  };
}
