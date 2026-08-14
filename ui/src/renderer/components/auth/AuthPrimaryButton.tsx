/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { type ButtonHTMLAttributes, type ReactNode } from 'react';

export interface AuthPrimaryButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  loading?: boolean;
  loadingLabel?: ReactNode;
  icon?: ReactNode;
}

const AuthPrimaryButton: React.FC<AuthPrimaryButtonProps> = ({
  children,
  loading = false,
  loadingLabel,
  icon,
  disabled,
  className,
  ...buttonProps
}) => (
  <button
    {...buttonProps}
    className={`flowy-auth-primary${className ? ` ${className}` : ''}`}
    disabled={disabled || loading}
    aria-busy={loading || undefined}
  >
    {loading && <span className='flowy-auth-spinner' aria-hidden='true' />}
    <span>{loading ? loadingLabel ?? children : children}</span>
    {!loading && icon && <span className='flowy-auth-primary__icon' aria-hidden='true'>{icon}</span>}
  </button>
);

export default AuthPrimaryButton;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
