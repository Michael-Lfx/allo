/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import FlowyLogo from '@renderer/components/brand/FlowyLogo';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { changeLanguage } from '@/renderer/services/i18n';
import { useNavigate } from 'react-router-dom';
import { motion, useReducedMotion } from 'framer-motion';
import { PreviewClose, PreviewOpen, Lock, User } from '@icon-park/react';
import AppLoader from '@renderer/components/layout/AppLoader';
import { useAuth } from '../../hooks/context/AuthContext';
import LanMesh from './LanMesh';
import './LoginPage.css';

type MessageState = {
  type: 'error' | 'success';
  text: string;
};

const REMEMBER_ME_KEY = 'rememberMe';
const REMEMBERED_USERNAME_KEY = 'rememberedUsername';
const REMEMBERED_PASSWORD_KEY = 'rememberedPassword';

// Simple obfuscation for stored credentials (not cryptographically secure, but prevents plain text storage)
const obfuscate = (text: string): string => {
  const encoded = btoa(encodeURIComponent(text));
  return encoded.split('').reverse().join('');
};

const deobfuscate = (text: string): string => {
  try {
    const reversed = text.split('').reverse().join('');
    return decodeURIComponent(atob(reversed));
  } catch {
    return '';
  }
};

const LoginPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const reduceMotion = useReducedMotion();
  const { status, login, setup, needsSetup } = useAuth();

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(false);
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [message, setMessage] = useState<MessageState | null>(null);
  const [loading, setLoading] = useState(false);

  const usernameRef = useRef<HTMLInputElement | null>(null);
  const passwordRef = useRef<HTMLInputElement | null>(null);
  const messageTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    document.body.classList.add('login-page-active');
    return () => {
      document.body.classList.remove('login-page-active');
      if (messageTimer.current) {
        window.clearTimeout(messageTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    document.title = t('login.pageTitle');
  }, [t]);

  useEffect(() => {
    document.documentElement.lang = i18n.language;
  }, [i18n.language]);

  useEffect(() => {
    const isRememberMe = localStorage.getItem(REMEMBER_ME_KEY) === 'true';
    if (isRememberMe) {
      const storedUsername = localStorage.getItem(REMEMBERED_USERNAME_KEY);
      const storedPassword = localStorage.getItem(REMEMBERED_PASSWORD_KEY);
      if (storedUsername) setUsername(deobfuscate(storedUsername));
      if (storedPassword) setPassword(deobfuscate(storedPassword));
      setRememberMe(true);
    }
    window.setTimeout(() => {
      usernameRef.current?.focus();
    }, 0);

    return () => {
      if (messageTimer.current) {
        window.clearTimeout(messageTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    if (status === 'authenticated') {
      void navigate('/guid', { replace: true });
    }
  }, [navigate, status]);

  const clearMessageLater = useCallback(() => {
    if (messageTimer.current) {
      window.clearTimeout(messageTimer.current);
    }
    messageTimer.current = window.setTimeout(() => {
      setMessage((prev) => (prev?.type === 'success' ? prev : null));
    }, 5000);
  }, []);

  const showMessage = useCallback(
    (next: MessageState) => {
      setMessage(next);
      if (next.type === 'error') {
        clearMessageLater();
      }
    },
    [clearMessageLater]
  );

  const supportedLanguages = useMemo<{ code: string; label: string }[]>(
    () => [
      { code: 'zh-CN', label: '简体中文' },
      { code: 'en-US', label: 'English' },
    ],
    []
  );

  const handleLanguageChange = useCallback((event: React.ChangeEvent<HTMLSelectElement>) => {
    const nextLanguage = event.target.value;
    changeLanguage(nextLanguage).catch((error: Error) => {
      console.error('Failed to change language:', error);
    });
  }, []);

  const handleSubmit = useCallback(
    async (event: React.FormEvent) => {
      event.preventDefault();
      const trimmedUsername = username.trim();

      if (!trimmedUsername || !password) {
        showMessage({ type: 'error', text: t('login.errors.empty') });
        return;
      }

      setLoading(true);
      setMessage(null);

      // First run: the typed credentials BECOME the initial admin. Otherwise
      // this is a normal sign-in.
      const result = needsSetup
        ? await setup({ username: trimmedUsername, password })
        : await login({ username: trimmedUsername, password, remember: rememberMe });

      if (result.success) {
        if (!needsSetup && rememberMe) {
          localStorage.setItem(REMEMBER_ME_KEY, 'true');
          localStorage.setItem(REMEMBERED_USERNAME_KEY, obfuscate(trimmedUsername));
          localStorage.setItem(REMEMBERED_PASSWORD_KEY, obfuscate(password));
        } else if (!needsSetup) {
          localStorage.removeItem(REMEMBER_ME_KEY);
          localStorage.removeItem(REMEMBERED_USERNAME_KEY);
          localStorage.removeItem(REMEMBERED_PASSWORD_KEY);
        }

        const successText = needsSetup ? t('login.setupSuccess') : t('login.success');
        showMessage({ type: 'success', text: successText });

        window.setTimeout(() => {
          void navigate('/guid', { replace: true });
        }, 600);
      } else {
        const errorText = (() => {
          switch (result.code) {
            case 'invalidCredentials':
              return t('login.errors.invalidCredentials');
            case 'tooManyAttempts':
              return t('login.errors.tooManyAttempts');
            case 'networkError':
              return t('login.errors.networkError');
            case 'serverError':
              return t('login.errors.serverError');
            case 'unknown':
            default:
              return result.message ?? t('login.errors.unknown');
          }
        })();

        showMessage({ type: 'error', text: errorText });
      }

      setLoading(false);
    },
    [login, setup, needsSetup, navigate, password, rememberMe, showMessage, t, username]
  );

  if (status === 'checking') {
    return <AppLoader />;
  }

  const panelTitle = needsSetup ? t('login.setupTitle') : t('login.welcomeTitle');
  const panelDesc = needsSetup ? t('login.setupSubtitle') : t('login.subtitle');

  return (
    <div className='login-page'>
      <motion.div
        className='login-page__shell'
        initial={reduceMotion ? false : { opacity: 0, scale: 0.97 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
      >
        <aside className='login-page__brand'>
          <LanMesh />
          <div className='login-page__brand-content'>
            <motion.div
              className='login-page__brand-logo'
              initial={reduceMotion ? false : { opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2, duration: 0.4 }}
            >
              <FlowyLogo size={36} title={t('login.brand')} />
            </motion.div>
            <motion.h2
              className='login-page__brand-title'
              initial={reduceMotion ? false : { opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3, duration: 0.4 }}
            >
              {t('login.brand')}
            </motion.h2>
          </div>
        </aside>

        <section className='login-page__panel'>
          <motion.div
            className='login-page__panel-inner'
            initial={reduceMotion ? false : { opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.12, duration: 0.4 }}
          >
            <label className='login-page__lang' htmlFor='lang-select'>
              <span className='login-page__visually-hidden'>{t('login.languageToggle')}</span>
              <select
                id='lang-select'
                className='login-page__lang-select'
                value={i18n.language}
                onChange={handleLanguageChange}
              >
                {supportedLanguages.map((lang) => (
                  <option key={lang.code} value={lang.code}>
                    {lang.label}
                  </option>
                ))}
              </select>
            </label>

            <h1 className='login-page__panel-title'>{panelTitle}</h1>
            <p className='login-page__panel-desc'>{panelDesc}</p>

            <form className='login-page__form' onSubmit={handleSubmit}>
              <div className='login-page__field'>
                <label className='login-page__label' htmlFor='username'>
                  {t('login.username')}
                </label>
                <div className='login-page__input-wrap'>
                  <span className='login-page__input-icon' aria-hidden='true'>
                    <User theme='outline' size={16} strokeWidth={3} />
                  </span>
                  <input
                    ref={usernameRef}
                    id='username'
                    name='username'
                    className='login-page__input'
                    placeholder={t('login.usernamePlaceholder')}
                    autoComplete='username'
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                    aria-required='true'
                  />
                </div>
              </div>

              <div className='login-page__field'>
                <label className='login-page__label' htmlFor='password'>
                  {t('login.password')}
                </label>
                <div className='login-page__input-wrap'>
                  <span className='login-page__input-icon' aria-hidden='true'>
                    <Lock theme='outline' size={16} strokeWidth={3} />
                  </span>
                  <input
                    ref={passwordRef}
                    id='password'
                    name='password'
                    type={passwordVisible ? 'text' : 'password'}
                    className='login-page__input'
                    placeholder={t('login.passwordPlaceholder')}
                    autoComplete={needsSetup ? 'new-password' : 'current-password'}
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                    aria-required='true'
                  />
                  <button
                    type='button'
                    className='login-page__toggle-password'
                    onClick={() => setPasswordVisible((prev) => !prev)}
                    aria-label={passwordVisible ? t('login.hidePassword') : t('login.showPassword')}
                  >
                    {passwordVisible ? (
                      <PreviewClose theme='outline' size={16} strokeWidth={3} />
                    ) : (
                      <PreviewOpen theme='outline' size={16} strokeWidth={3} />
                    )}
                  </button>
                </div>
              </div>

              {!needsSetup && (
                <div className='login-page__checkbox'>
                  <input
                    type='checkbox'
                    id='remember-me'
                    checked={rememberMe}
                    onChange={(event) => setRememberMe(event.target.checked)}
                  />
                  <label htmlFor='remember-me'>{t('login.rememberMe')}</label>
                </div>
              )}

              <motion.button
                type='submit'
                className='login-page__submit'
                disabled={loading}
                whileHover={loading || reduceMotion ? undefined : { y: -1 }}
                whileTap={loading || reduceMotion ? undefined : { scale: 0.985 }}
              >
                {loading && (
                  <svg className='login-page__spinner' viewBox='0 0 24 24' width='18' height='18'>
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
                  {loading
                    ? t('login.submitting')
                    : needsSetup
                      ? t('login.setupSubmit')
                      : t('login.submit')}
                </span>
              </motion.button>

              <div
                role='alert'
                aria-live='polite'
                className={`login-page__message ${message ? `login-page__message--${message.type}` : 'login-page__message--empty'}`}
              >
                {message?.text ?? '\u00A0'}
              </div>
            </form>

            <div className='login-page__footer'>
              <span>{t('login.footerPrimary')}</span>
              <span className='login-page__footer-divider' aria-hidden='true'>
                ·
              </span>
              <span>{t('login.footerSecondary')}</span>
            </div>
          </motion.div>
        </section>
      </motion.div>
    </div>
  );
};

export default LoginPage;
