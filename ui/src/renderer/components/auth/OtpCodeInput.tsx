/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, {
  forwardRef,
  useCallback,
  useLayoutEffect,
  useEffect,
  useRef,
  type ClipboardEvent,
  type InputHTMLAttributes,
  type KeyboardEvent,
} from 'react';
import { EMAIL_OTP_LENGTH, normalizeOtpCode } from '@renderer/hooks/auth/useEmailOtpLogin';

export interface OtpCodeInputProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange' | 'onComplete' | 'onSubmit'> {
  value: string;
  onChange: (value: string) => void;
  onComplete?: (value: string) => void;
  onSubmit?: (value: string) => void;
  label: string;
  compact?: boolean;
  focusOnErrorReset?: boolean;
}

const OtpCodeInput = forwardRef<HTMLInputElement, OtpCodeInputProps>(function OtpCodeInput(
  {
    value,
    onChange,
    onComplete,
    onSubmit,
    label,
    compact = false,
    focusOnErrorReset = false,
    id = 'auth-otp',
    className,
    'aria-describedby': ariaDescribedBy,
    disabled,
    ...inputProps
  },
  ref
) {
  const lastCompletedRef = useRef<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const previousValueRef = useRef(normalizeOtpCode(value));
  const normalizedValue = normalizeOtpCode(value);
  const errorId = `${id}-error`;
  const assignInputRef = useCallback((node: HTMLInputElement | null) => {
    inputRef.current = node;
    if (typeof ref === 'function') ref(node);
    else if (ref) ref.current = node;
  }, [ref]);

  useLayoutEffect(() => {
    const previousValue = previousValueRef.current;
    previousValueRef.current = normalizedValue;

    if (
      !focusOnErrorReset
      || previousValue.length !== EMAIL_OTP_LENGTH
      || normalizedValue.length !== 0
      || disabled
    ) {
      return;
    }

    const input = inputRef.current;
    if (!input) return;
    input.focus({ preventScroll: true });
    input.setSelectionRange(0, 0);
  }, [disabled, focusOnErrorReset, normalizedValue]);

  useEffect(() => {
    if (normalizedValue.length !== EMAIL_OTP_LENGTH) {
      lastCompletedRef.current = null;
      return;
    }
    if (lastCompletedRef.current === normalizedValue) return;
    lastCompletedRef.current = normalizedValue;
    onComplete?.(normalizedValue);
  }, [normalizedValue, onComplete]);

  const handleChange = (nextValue: string) => {
    onChange(normalizeOtpCode(nextValue));
  };

  const handlePaste = (event: ClipboardEvent<HTMLInputElement>) => {
    event.preventDefault();
    handleChange(event.clipboardData.getData('text'));
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter' && normalizedValue.length === EMAIL_OTP_LENGTH) {
      event.preventDefault();
      onSubmit?.(normalizedValue);
    }
  };

  return (
    <div className={`flowy-otp${compact ? ' flowy-otp--compact' : ''}${className ? ` ${className}` : ''}`}>
      <div className='flowy-otp__cells' aria-hidden='true'>
        {Array.from({ length: EMAIL_OTP_LENGTH }, (_, index) => (
          <span
            className={`flowy-otp__cell${index === 3 ? ' flowy-otp__cell--group-start' : ''}${
              normalizedValue[index] ? ' is-filled' : ''
            }${index === normalizedValue.length ? ' is-active' : ''}`}
            key={index}
          >
            {normalizedValue[index] ?? ''}
          </span>
        ))}
      </div>
      <input
        {...inputProps}
        ref={assignInputRef}
        id={id}
        type='text'
        inputMode='numeric'
        autoComplete='one-time-code'
        maxLength={EMAIL_OTP_LENGTH}
        pattern='[0-9]*'
        name={inputProps.name ?? 'one-time-code'}
        value={normalizedValue}
        onChange={(event) => handleChange(event.target.value)}
        onPaste={handlePaste}
        onKeyDown={handleKeyDown}
        disabled={disabled}
        aria-label={label}
        aria-describedby={ariaDescribedBy || undefined}
        aria-invalid={inputProps['aria-invalid']}
      />
      <span className='flowy-auth-visually-hidden' id={errorId} />
    </div>
  );
});

export default OtpCodeInput;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
