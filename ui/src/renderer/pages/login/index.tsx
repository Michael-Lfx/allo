import FlowyLogo from '@renderer/components/brand/FlowyLogo';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { changeLanguage } from '@/renderer/services/i18n';
import { useNavigate } from 'react-router-dom';
import { PreviewClose, PreviewOpen, Lock, User } from '@icon-park/react';
import AuthField from '@renderer/components/auth/AuthField';
import AuthPrimaryButton from '@renderer/components/auth/AuthPrimaryButton';
import AuthShell from '@renderer/components/auth/AuthShell';
import AuthStatusBar from '@renderer/components/auth/AuthStatusBar';
import type { IntentFieldPhase } from '@renderer/components/auth/authTypes';
import { useAuth } from '../../hooks/context/AuthContext';
import { useCloudAuth } from '../../hooks/context/CloudAuthContext';
import { resolvePostLocalAuthPath } from '@renderer/utils/auth/authGate';
import './LoginPage.css';
import '@renderer/components/auth/auth.css';

type MessageState = {
  type: 'error' | 'success';
  text: string;
};

const REMEMBER_ME_KEY = 'rememberMe';
const REMEMBERED_USERNAME_KEY = 'rememberedUsername';
const LEGACY_REMEMBERED_PASSWORD_KEY = 'rememberedPassword';

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
  const { status, login, setup, needsSetup } = useAuth();
  const { status: cloudStatus } = useCloudAuth();

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(false);
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [message, setMessage] = useState<MessageState | null>(null);
  const [loading, setLoading] = useState(false);
  const [intentPhase, setIntentPhase] = useState<IntentFieldPhase>('idle');
  const navigatingRef = useRef(false);
  const usernameRef = useRef<HTMLInputElement | null>(null);
  const messageTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    document.body.classList.add('login-page-active');
    document.title = t('login.pageTitle');
    document.documentElement.lang = i18n.language;
    return () => {
      document.body.classList.remove('login-page-active');
      if (messageTimer.current) window.clearTimeout(messageTimer.current);
    };
  }, [i18n.language, t]);

  useEffect(() => {
    localStorage.removeItem(LEGACY_REMEMBERED_PASSWORD_KEY);
    const isRememberMe = localStorage.getItem(REMEMBER_ME_KEY) === 'true';
    if (isRememberMe) {
      const storedUsername = localStorage.getItem(REMEMBERED_USERNAME_KEY);
      if (storedUsername) setUsername(deobfuscate(storedUsername));
      setRememberMe(true);
    }
    window.setTimeout(() => usernameRef.current?.focus(), 0);
  }, []);

  const goAfterLocalAuth = useCallback(() => {
    if (navigatingRef.current) return;
    navigatingRef.current = true;
    void navigate(resolvePostLocalAuthPath(cloudStatus === 'authenticated'), { replace: true });
  }, [cloudStatus, navigate]);

  useEffect(() => {
    if (status === 'authenticated') goAfterLocalAuth();
  }, [goAfterLocalAuth, status]);

  const clearMessageLater = useCallback(() => {
    if (messageTimer.current) window.clearTimeout(messageTimer.current);
    messageTimer.current = window.setTimeout(() => {
      setMessage((previous) => (previous?.type === 'success' ? previous : null));
    }, 5000);
  }, []);

  const showMessage = useCallback((next: MessageState) => {
    setMessage(next);
    if (next.type === 'error') clearMessageLater();
  }, [clearMessageLater]);

  const supportedLanguages = useMemo(
    () => [
      { code: 'zh-CN', label: '简体中文' },
      { code: 'en-US', label: 'English' },
    ],
    []
  );

  const handleLanguageChange = useCallback((event: React.ChangeEvent<HTMLSelectElement>) => {
    void changeLanguage(event.target.value).catch((error: Error) => {
      console.error('Failed to change language:', error);
    });
  }, []);

  const handleSubmit = useCallback(async (event: React.FormEvent) => {
    event.preventDefault();
    const trimmedUsername = username.trim();
    if (!trimmedUsername || !password) {
      setIntentPhase('error');
      showMessage({ type: 'error', text: t('login.errors.empty') });
      return;
    }

    setLoading(true);
    setIntentPhase('verifying');
    setMessage(null);
    const result = needsSetup
      ? await setup({ username: trimmedUsername, password })
      : await login({ username: trimmedUsername, password });

    if (result.success) {
      if (!needsSetup && rememberMe) {
        localStorage.setItem(REMEMBER_ME_KEY, 'true');
        localStorage.setItem(REMEMBERED_USERNAME_KEY, obfuscate(trimmedUsername));
      } else if (!needsSetup) {
        localStorage.removeItem(REMEMBER_ME_KEY);
        localStorage.removeItem(REMEMBERED_USERNAME_KEY);
      }
      localStorage.removeItem(LEGACY_REMEMBERED_PASSWORD_KEY);
      setIntentPhase('success');
      showMessage({ type: 'success', text: needsSetup ? t('login.setupSuccess') : t('login.success') });
      goAfterLocalAuth();
    } else {
      const errorText = (() => {
        switch (result.code) {
          case 'invalidCredentials': return t('login.errors.invalidCredentials');
          case 'tooManyAttempts': return t('login.errors.tooManyAttempts');
          case 'networkError': return t('login.errors.networkError');
          case 'serverError': return t('login.errors.serverError');
          case 'csrfError':
          case 'unknown':
          default: return result.message ?? t('login.errors.unknown');
        }
      })();
      setIntentPhase('error');
      showMessage({ type: 'error', text: errorText });
    }
    setLoading(false);
  }, [goAfterLocalAuth, login, needsSetup, password, rememberMe, setup, showMessage, t, username]);

  if (status === 'checking') return null;

  const panelTitle = needsSetup ? t('login.setupTitle') : t('login.welcomeTitle');
  const panelDesc = needsSetup ? t('login.setupSubtitle') : t('login.subtitle');

  return (
    <AuthShell
      mode='local'
      phase={intentPhase}
      inputEnergy={(username.trim() ? 0.5 : 0) + (password ? 0.5 : 0)}
      brandTitle={t('login.brand')}
      brandTagline={t('login.brandTagline')}
      brandFlow={t('login.brandFlow')}
      brandLogo={<FlowyLogo size={36} title={t('login.brand')} />}
      brandMeta={t('login.footerSecondary')}
      footer={
        <>
          <span className='flowy-auth-footer__group'>{t('login.footerPrimary')}</span>
          <span className='flowy-auth-footer__group'>{t('login.footerSecondary')}</span>
        </>
      }
      className='login-page'
    >
      <div className='login-page__language'>
        <label className='flowy-auth-visually-hidden' htmlFor='lang-select'>
          {t('login.languageToggle')}
        </label>
        <select
          id='lang-select'
          className='login-page__lang-select'
          value={i18n.language}
          onChange={handleLanguageChange}
        >
          {supportedLanguages.map((language) => (
            <option key={language.code} value={language.code}>{language.label}</option>
          ))}
        </select>
      </div>

      <h1 className='flowy-auth-heading'>{panelTitle}</h1>
      <p className='flowy-auth-description'>{panelDesc}</p>

      <form className='flowy-auth-form login-page__form' onSubmit={handleSubmit}>
        <AuthField
          ref={usernameRef}
          id='username'
          name='username'
          type='text'
          label={t('login.username')}
          placeholder={t('login.usernamePlaceholder')}
          autoComplete='username'
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          onFocus={() => !loading && setIntentPhase('input')}
          aria-required='true'
          leading={<User theme='outline' size={16} strokeWidth={3} />}
        />

        <AuthField
          id='password'
          name='password'
          type={passwordVisible ? 'text' : 'password'}
          label={t('login.password')}
          placeholder={t('login.passwordPlaceholder')}
          autoComplete={needsSetup ? 'new-password' : 'current-password'}
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          onFocus={() => !loading && setIntentPhase('input')}
          aria-required='true'
          trailing={
            <button
              type='button'
              className='login-page__toggle-password'
              onClick={() => setPasswordVisible((previous) => !previous)}
              aria-label={passwordVisible ? t('login.hidePassword') : t('login.showPassword')}
            >
              {passwordVisible ? <PreviewClose theme='outline' size={16} strokeWidth={3} /> : <PreviewOpen theme='outline' size={16} strokeWidth={3} />}
            </button>
          }
          leading={<Lock theme='outline' size={16} strokeWidth={3} />}
        />

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

        <AuthPrimaryButton
          type='submit'
          loading={loading}
          loadingLabel={t('login.submitting')}
          icon={<span>→</span>}
        >
          {needsSetup ? t('login.setupSubmit') : t('login.submit')}
        </AuthPrimaryButton>

        {message && <AuthStatusBar tone={message.type}>{message.text}</AuthStatusBar>}
      </form>
    </AuthShell>
  );
};

export default LoginPage;
