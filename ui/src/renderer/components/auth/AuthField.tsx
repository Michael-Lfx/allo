/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { forwardRef, type InputHTMLAttributes, type ReactNode } from 'react';

export interface AuthFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: ReactNode;
  error?: ReactNode;
  hint?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
}

const AuthField = forwardRef<HTMLInputElement, AuthFieldProps>(function AuthField(
  {
    id,
    label,
    error,
    hint,
    leading,
    trailing,
    className,
    'aria-describedby': ariaDescribedBy,
    ...inputProps
  },
  ref
) {
  const errorId = `${id}-error`;
  const hintId = `${id}-hint`;
  const describedBy = [ariaDescribedBy, hint ? hintId : undefined, error ? errorId : undefined]
    .filter(Boolean)
    .join(' ') || undefined;

  return (
    <div className='flowy-auth-field'>
      <label className='flowy-auth-label' htmlFor={id}>
        {label}
      </label>
      <div className={`flowy-auth-input-wrap${className ? ` ${className}` : ''}`}>
        {leading && <span className='flowy-auth-input-leading' aria-hidden='true'>{leading}</span>}
        <input
          {...inputProps}
          ref={ref}
          id={id}
          className='flowy-auth-input'
          aria-describedby={describedBy}
          aria-invalid={error ? true : undefined}
        />
        {trailing && <span className='flowy-auth-input-trailing'>{trailing}</span>}
      </div>
      {hint && <p className='flowy-auth-field__hint' id={hintId}>{hint}</p>}
      {error && <p className='flowy-auth-field__error' id={errorId} role='alert'>{error}</p>}
    </div>
  );
});

export default AuthField;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
