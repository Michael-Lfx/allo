/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { type ReactNode } from 'react';
import type { AuthStatusTone } from './authTypes';

export interface AuthStatusBarProps {
  id?: string;
  tone?: AuthStatusTone;
  children?: ReactNode;
  action?: ReactNode;
  reserveActionSpace?: boolean;
  live?: 'off' | 'polite' | 'assertive';
  className?: string;
}

const AuthStatusBar: React.FC<AuthStatusBarProps> = ({
  id,
  tone = 'info',
  children,
  action,
  reserveActionSpace = false,
  live = 'polite',
  className,
}) => {
  const hasAction = action !== undefined && action !== null && action !== false;
  const shouldRenderActionSlot = reserveActionSpace || hasAction;

  return (
    <div
      id={id}
      className={`flowy-auth-status flowy-auth-status--${tone}${reserveActionSpace ? ' flowy-auth-status--reserve-action' : ''}${className ? ` ${className}` : ''}`}
      role={live === 'off' ? undefined : tone === 'error' ? 'alert' : 'status'}
      aria-live={live === 'off' ? undefined : live}
    >
      <span className='flowy-auth-status__mark' aria-hidden='true' />
      <span className='flowy-auth-status__message'>{children}</span>
      {shouldRenderActionSlot && (
        <span
          className='flowy-auth-status__action'
          data-empty={!hasAction ? 'true' : undefined}
          aria-hidden={!hasAction}
        >
          {action}
        </span>
      )}
    </div>
  );
};

export default AuthStatusBar;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
