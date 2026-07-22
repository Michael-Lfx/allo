/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import appLogo from '@renderer/assets/logo.svg';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { motion } from 'framer-motion';
import WindowControls from '@renderer/components/layout/WindowControls';
import { ipcBridge } from '@/common';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { isDesktopShell, isMacOS } from '@renderer/utils/platform';
import DotMap from './DotMap';
import './CloudLoginPage.css';
import '@renderer/components/layout/Titlebar/titlebar.css';

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
  const [resendCooldown, setResendCooldown] = useState(0);
  const justLoggedInRef = useRef(false);
  const initStartedRef = useRef(false);
  const emailRef = useRef<HTMLInputElement | null>(null);
  const otpRef = useRef<HTMLInputElement | null>(null);

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

  useEffect(() => {
    if (resendCooldown <= 0) return;
    const timer = window.setTimeout(() => {
      setResendCooldown((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [resendCooldown]);

  const startResendCooldown = useCallback(() => {
    setResendCooldown(60);
  }, []);

  const handleSendCode = useCallback(async () => {
    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      showMessage({ type: 'error', text: t('cloudLogin.login.emailRequired') });
      return;
    }
    if (resendCooldown > 0 || sendingCode) return;

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
        startResendCooldown();
        showMessage({ type: 'info', text: t('cloudLogin.login.codeSent') });
        window.setTimeout(() => otpRef.current?.focus(), 0);
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
  }, [email, ensureSession, refresh, resendCooldown, sendingCode, showMessage, startResendCooldown, t]);

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
      setResendCooldown(0);
      initStartedRef.current = false;
      showMessage(null);
      await ensureSession();
    } finally {
      setLoggingIn(false);
    }
  }, [logout, ensureSession, showMessage]);

  // Route Suspense already shows fullscreen AppLoader; avoid a second viewport swap.
  if (status === 'checking') {
    return null;
  }

  const busy = sendingCode || loggingIn;
  const isSignedIn = Boolean(whoami?.authenticated);
  const showWindowControls = isDesktopShell() && !isMacOS();
  const canSendCode = Boolean(email.trim()) && !busy && resendCooldown <= 0;
  const sendCodeLabel = sendingCode
    ? t('cloudLogin.login.sendingCode')
    : resendCooldown > 0
      ? t('cloudLogin.login.resendIn', { seconds: resendCooldown })
      : t('cloudLogin.login.sendCode');

  return (
    <div className='cloud-login-page'>
      {showWindowControls && (
        <div className='cloud-login-chrome' data-tauri-drag-region>
          <div className='cloud-login-chrome__spacer' data-tauri-drag-region />
          <WindowControls />
        </div>
      )}
      <motion.div
        className='cloud-login-shell'
        initial={{ opacity: 0, scale: 0.97 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
      >
        <aside className='cloud-login-brand' aria-hidden={false}>
          <div className='cloud-login-brand__map'>
            <DotMap />
          </div>
          <div className='cloud-login-brand__content'>
            <motion.div
              className='cloud-login-brand__logo'
              initial={{ opacity: 0, y: -12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.25, duration: 0.4 }}
            >
              <img src={appLogo} alt='' />
            </motion.div>
            <motion.h2
              className='cloud-login-brand__title'
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.35, duration: 0.4 }}
            >
              {t('cloudLogin.brand')}
            </motion.h2>
            <motion.p
              className='cloud-login-brand__tagline'
              initial={{ opacity: 0, y: -8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.42, duration: 0.4 }}
            >
              {t('cloudLogin.brandTagline')}
            </motion.p>
            <motion.ul
              className='cloud-login-brand__points'
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.5, duration: 0.4 }}
            >
              <li>{t('cloudLogin.brandPoints.workspace')}</li>
              <li>{t('cloudLogin.brandPoints.agents')}</li>
              <li>{t('cloudLogin.brandPoints.ready')}</li>
            </motion.ul>
          </div>
        </aside>

        <section className='cloud-login-panel'>
          <motion.div
            className='cloud-login-panel__inner'
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.15, duration: 0.4 }}
          >
            {isSignedIn ? (
              <>
                <h1 className='cloud-login-panel__title'>{t('cloudLogin.account.signedIn')}</h1>
                <p className='cloud-login-panel__desc'>{t('cloudLogin.accountSubtitle')}</p>
                <div className='cloud-login-signed-actions'>
                  {(whoami?.email || whoami?.username) && (
                    <p className='cloud-login-signed-email'>{whoami.email ?? whoami.username}</p>
                  )}
                  <motion.button
                    type='button'
                    className='cloud-login-submit'
                    whileHover={{ y: -1 }}
                    whileTap={{ scale: 0.985 }}
                    onClick={() => navigate('/guid')}
                  >
                    {t('cloudLogin.login.continue')}
                  </motion.button>
                  <button
                    type='button'
                    className='cloud-login-submit cloud-login-submit--ghost'
                    disabled={loggingIn}
                    onClick={handleLogout}
                  >
                    {t('cloudLogin.login.logout')}
                  </button>
                </div>
              </>
            ) : (
              <>
                <h1 className='cloud-login-panel__title'>
                  {codeSent ? t('cloudLogin.login.stepOtp') : t('cloudLogin.welcomeTitle')}
                </h1>
                <p className='cloud-login-panel__desc'>
                  {codeSent
                    ? t('cloudLogin.login.otpSentTo', { email: email.trim() })
                    : t('cloudLogin.welcomeDesc')}
                </p>

                <form
                  className='cloud-login-form'
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (!codeSent) {
                      void handleSendCode();
                      return;
                    }
                    void handleLogin();
                  }}
                >
                  {!codeSent ? (
                    <div className='cloud-login-field'>
                      <label className='cloud-login-label' htmlFor='cloud-email'>
                        {t('cloudLogin.login.emailLabel')}
                      </label>
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
                        aria-required='true'
                      />
                    </div>
                  ) : (
                    <div className='cloud-login-field'>
                      <label className='cloud-login-label' htmlFor='cloud-otp'>
                        {t('cloudLogin.login.otpLabel')}
                      </label>
                      <input
                        ref={otpRef}
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
                        aria-required='true'
                      />
                      <div className='cloud-login-otp-actions'>
                        <button
                          type='button'
                          className='cloud-login-text-btn'
                          disabled={busy}
                          onClick={() => {
                            setCodeSent(false);
                            setOtp('');
                            setMessage(null);
                            window.setTimeout(() => emailRef.current?.focus(), 0);
                          }}
                        >
                          {t('cloudLogin.login.back')}
                        </button>
                        <button
                          type='button'
                          className='cloud-login-text-btn'
                          disabled={!canSendCode}
                          onClick={() => void handleSendCode()}
                        >
                          {sendCodeLabel}
                        </button>
                      </div>
                    </div>
                  )}

                  <motion.button
                    type='submit'
                    className='cloud-login-submit'
                    disabled={busy || (!codeSent ? !email.trim() : !otp.trim())}
                    whileHover={busy ? undefined : { y: -1 }}
                    whileTap={busy ? undefined : { scale: 0.985 }}
                  >
                    {(loggingIn || sendingCode) && (
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
                    <span>
                      {loggingIn
                        ? t('cloudLogin.login.loggingIn')
                        : sendingCode
                          ? t('cloudLogin.login.sendingCode')
                          : codeSent
                            ? t('cloudLogin.login.verify')
                            : t('cloudLogin.login.sendCode')}
                    </span>
                  </motion.button>

                  <div
                    role='alert'
                    aria-live='polite'
                    className={`cloud-login-message ${message ? `cloud-login-message--${message.type}` : 'cloud-login-message--empty'}`}
                  >
                    {message?.text ?? '\u00A0'}
                  </div>
                </form>

                <div className='cloud-login-footer'>
                  <span>{t('cloudLogin.footerPrimary')}</span>
                  <span className='cloud-login-footer__dot'>·</span>
                  <span>{t('cloudLogin.footerSecondary')}</span>
                </div>
              </>
            )}
          </motion.div>
        </section>
      </motion.div>
    </div>
  );
};

export default CloudLoginPage;
