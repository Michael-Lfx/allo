/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import appLogo from '@renderer/assets/logo.svg';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import AppLoader from '@renderer/components/layout/AppLoader';
import { ipcBridge } from '@/common';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import './CloudLoginPage.css';

type MessageState = { type: 'error' | 'success' | 'info'; text: string };

const CloudLoginPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status, whoami, refresh, logout } = useCloudAuth();

  const [pendingId, setPendingId] = useState<string | null>(null);
  const [codeSent, setCodeSent] = useState(false);
  const [email, setEmail] = useState('');
  const [otp, setOtp] = useState('');
  const [sendingCode, setSendingCode] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [message, setMessage] = useState<MessageState | null>(null);
  const justLoggedInRef = useRef(false);
  const initStartedRef = useRef(false);
  const emailRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    document.body.classList.add('login-page-active');
    document.title = t('cloudLogin.pageTitle');
    return () => {
      document.body.classList.remove('login-page-active');
    };
  }, [t]);

  const showMessage = useCallback((next: MessageState | null) => {
    setMessage(next);
  }, []);

  const ensureSession = useCallback(async (): Promise<string | null> => {
    if (pendingId) return pendingId;
    try {
      const res = await ipcBridge.cloud.loginStart.invoke({ method: 'email_otp' });
      setPendingId(res.pendingId);
      return res.pendingId;
    } catch (e) {
      showMessage({ type: 'error', text: String(e) });
      return null;
    }
  }, [pendingId, showMessage]);

  useEffect(() => {
    if (whoami?.authenticated || initStartedRef.current) return;
    initStartedRef.current = true;
    void ensureSession();
    window.setTimeout(() => emailRef.current?.focus(), 0);
  }, [whoami?.authenticated, ensureSession]);

  useEffect(() => {
    if (justLoggedInRef.current && status === 'authenticated') {
      navigate('/guid', { replace: true });
    }
  }, [status, navigate]);

  const handleSendCode = useCallback(async () => {
    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      showMessage({ type: 'error', text: t('cloudLogin.login.emailRequired') });
      return;
    }

    setSendingCode(true);
    showMessage(null);
    try {
      const sessionId = await ensureSession();
      if (!sessionId) return;

      const res = await ipcBridge.cloud.loginContinue.invoke({
        pendingId: sessionId,
        input: { type: 'email', address: trimmedEmail },
      });

      if (res.status === 'pending') {
        setPendingId(res.pendingId);
        setCodeSent(true);
        showMessage({ type: 'info', text: t('cloudLogin.login.codeSent') });
      } else if (res.status === 'success') {
        justLoggedInRef.current = true;
        showMessage({ type: 'success', text: t('cloudLogin.login.successRedirect') });
        await refresh();
      } else {
        showMessage({ type: 'error', text: res.error ?? t('cloudLogin.errors.unknown') });
      }
    } catch (e) {
      showMessage({ type: 'error', text: String(e) });
    } finally {
      setSendingCode(false);
    }
  }, [email, ensureSession, refresh, showMessage, t]);

  const handleLogin = useCallback(async () => {
    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      showMessage({ type: 'error', text: t('cloudLogin.login.emailRequired') });
      return;
    }
    if (!otp.trim()) {
      showMessage({ type: 'error', text: t('cloudLogin.login.otpRequired') });
      return;
    }
    if (!pendingId) {
      showMessage({ type: 'error', text: t('cloudLogin.login.sendCodeFirst') });
      return;
    }

    setLoggingIn(true);
    showMessage(null);
    try {
      const res = await ipcBridge.cloud.loginContinue.invoke({
        pendingId,
        input: { type: 'otp_code', code: otp.trim() },
      });

      if (res.status === 'success') {
        justLoggedInRef.current = true;
        setOtp('');
        showMessage({ type: 'success', text: t('cloudLogin.login.successRedirect') });
        await refresh();
      } else if (res.status === 'pending') {
        setPendingId(res.pendingId);
        showMessage({ type: 'info', text: res.message });
      } else {
        showMessage({ type: 'error', text: res.error ?? t('cloudLogin.errors.invalidCode') });
        setCodeSent(false);
        setPendingId(null);
        initStartedRef.current = false;
        void ensureSession();
      }
    } catch (e) {
      showMessage({ type: 'error', text: String(e) });
    } finally {
      setLoggingIn(false);
    }
  }, [email, otp, pendingId, refresh, ensureSession, showMessage, t]);

  const handleLogout = useCallback(async () => {
    setLoggingIn(true);
    try {
      await logout();
      setEmail('');
      setOtp('');
      setPendingId(null);
      setCodeSent(false);
      initStartedRef.current = false;
      showMessage(null);
      await ensureSession();
    } finally {
      setLoggingIn(false);
    }
  }, [logout, ensureSession]);

  if (status === 'checking') {
    return <AppLoader />;
  }

  const busy = sendingCode || loggingIn;

  return (
    <div className='cloud-login-page'>
      <div className='cloud-login-card'>
        <div className='cloud-login-logo'>
          <img src={appLogo} alt={t('cloudLogin.brand')} />
        </div>

        {whoami?.authenticated ? (
          <div className='cloud-login-signed-actions'>
            {(whoami.email || whoami.username) && (
              <p className='cloud-login-signed-email'>{whoami.email ?? whoami.username}</p>
            )}
            <button type='button' className='cloud-login-submit' onClick={() => navigate('/guid')}>
              {t('cloudLogin.login.continue')}
            </button>
            <button
              type='button'
              className='cloud-login-submit cloud-login-submit--ghost'
              disabled={loggingIn}
              onClick={handleLogout}
            >
              {t('cloudLogin.login.logout')}
            </button>
          </div>
        ) : (
          <form
            className='cloud-login-form'
            onSubmit={(event) => {
              event.preventDefault();
              void handleLogin();
            }}
          >
            <input
              ref={emailRef}
              id='cloud-email'
              type='email'
              className='cloud-login-input'
              placeholder={t('cloudLogin.login.emailPlaceholder')}
              autoComplete='email'
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              disabled={busy}
            />

            <div className='cloud-login-otp-row'>
              <input
                id='cloud-otp'
                type='text'
                inputMode='numeric'
                className='cloud-login-input'
                placeholder={t('cloudLogin.login.otpPlaceholder')}
                autoComplete='one-time-code'
                maxLength={8}
                value={otp}
                onChange={(event) => setOtp(event.target.value.replace(/\D/g, ''))}
                disabled={busy}
              />
              <button
                type='button'
                className='cloud-login-send-btn'
                disabled={busy || !email.trim()}
                onClick={() => void handleSendCode()}
              >
                {sendingCode ? t('cloudLogin.login.sendingCode') : t('cloudLogin.login.sendCode')}
              </button>
            </div>

            <button type='submit' className='cloud-login-submit' disabled={busy || !codeSent}>
              {loggingIn && (
                <svg className='cloud-login-spinner' viewBox='0 0 24 24' width='18' height='18'>
                  <circle
                    cx='12'
                    cy='12'
                    r='10'
                    stroke='currentColor'
                    strokeWidth='3'
                    fill='none'
                    strokeDasharray='50'
                    strokeDashoffset='25'
                    strokeLinecap='round'
                  />
                </svg>
              )}
              <span>{loggingIn ? t('cloudLogin.login.loggingIn') : t('cloudLogin.login.submit')}</span>
            </button>

            {message && (
              <div
                role='alert'
                className={`cloud-login-message cloud-login-message--${message.type}`}
              >
                {message.text}
              </div>
            )}
          </form>
        )}
      </div>
    </div>
  );
};

export default CloudLoginPage;
