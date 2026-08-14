/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { EMAIL_OTP_COOLDOWN_SECONDS } from '@renderer/hooks/auth/useEmailOtpLogin';
import AuthStatusBar from './AuthStatusBar';

interface AuthCooldownHintProps {
  seconds: number;
  children: ReactNode;
  showVisual?: boolean;
  className?: string;
}

const AuthCooldownHint: React.FC<AuthCooldownHintProps> = ({ seconds, children, showVisual = true, className }) => {
  const { t } = useTranslation();
  const previousSecondsRef = useRef<number | null>(null);
  const [announcement, setAnnouncement] = useState('');

  useEffect(() => {
    const previous = previousSecondsRef.current;
    if (seconds === EMAIL_OTP_COOLDOWN_SECONDS && (previous === null || previous === 0)) {
      setAnnouncement(t('cloudLogin.login.cooldownStarted'));
    } else if (seconds === 0 && previous !== null && previous > 0) {
      setAnnouncement(t('cloudLogin.login.cooldownReady'));
    }
    previousSecondsRef.current = seconds;
  }, [seconds, t]);

  return (
    <>
      {showVisual && seconds > 0 && (
        <AuthStatusBar tone='warning' live='off' className={className}>
          {children}
        </AuthStatusBar>
      )}
      <span className='flowy-auth-visually-hidden' aria-live='polite'>
        {announcement}
      </span>
    </>
  );
};

export default AuthCooldownHint;
