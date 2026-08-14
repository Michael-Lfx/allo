import appLogo from '@renderer/assets/logo.svg';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Refresh } from '@icon-park/react';
import WindowControls from '@renderer/components/layout/WindowControls';
import AuthCooldownHint from '@renderer/components/auth/AuthCooldownHint';
import AuthField from '@renderer/components/auth/AuthField';
import AuthPrimaryButton from '@renderer/components/auth/AuthPrimaryButton';
import AuthShell from '@renderer/components/auth/AuthShell';
import AuthStatusBar from '@renderer/components/auth/AuthStatusBar';
import OtpCodeInput from '@renderer/components/auth/OtpCodeInput';
import { getOtpBlueprintCheckpoint } from '@renderer/components/auth/blueprintScene';
import type {
  BlueprintRouteStep,
  CloudActivationLevel,
  IntentFieldPhase,
} from '@renderer/components/auth/authTypes';
import useEmailOtpLogin, { EMAIL_OTP_LENGTH } from '@renderer/hooks/auth/useEmailOtpLogin';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { isDesktopShell, isMacOS } from '@renderer/utils/platform';
import { preloadCommercialPathChunks, preloadGuidPathChunk } from '@renderer/utils/motion/flowyMotion';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import type { CloudAuthStatus } from '@renderer/hooks/context/CloudAuthContext';
import './CloudLoginPage.css';
import '@renderer/components/auth/auth.css';
import '@renderer/components/layout/Titlebar/titlebar.css';

const resolveIntentPhase = (
  step: 'email' | 'otp',
  phase: ReturnType<typeof useEmailOtpLogin>['phase'],
  failureKind: ReturnType<typeof useEmailOtpLogin>['failureKind'],
  hasInput: boolean
): IntentFieldPhase => {
  if (phase === 'success') return 'success';
  if (phase === 'session-expired' || failureKind === 'invalid-code' || failureKind === 'unknown') return 'error';
  if (phase === 'transport-error' || failureKind === 'transport' || failureKind === 'verification-pending') return 'warning';
  if (phase === 'verifying') return 'verifying';
  if (step === 'otp') return 'code-sent';
  return hasInput || phase === 'sending' ? 'input' : 'idle';
};

interface CloudLoginFlowProps {
  status: Exclude<CloudAuthStatus, 'checking'>;
  whoami: ReturnType<typeof useCloudAuth>['whoami'];
  logout: ReturnType<typeof useCloudAuth>['logout'];
  onSuccess: () => Promise<void>;
}

const preventPlaceholderNavigation = (event: React.MouseEvent<HTMLAnchorElement>) => {
  event.preventDefault();
};

const CloudLoginLegalLinks: React.FC = () => {
  const { t } = useTranslation();

  return (
    <>
      <a
        className='flowy-auth-brand__legal-link'
        href='#'
        aria-disabled='true'
        onClick={preventPlaceholderNavigation}
      >
        {t('cloudLogin.legal.privacy')}
      </a>
      <span className='flowy-auth-brand__legal-separator' aria-hidden='true'>{t('cloudLogin.legal.and')}</span>
      <a
        className='flowy-auth-brand__legal-link'
        href='#'
        aria-disabled='true'
        onClick={preventPlaceholderNavigation}
      >
        {t('cloudLogin.legal.terms')}
      </a>
    </>
  );
};

interface CloudLoginTransitionProps {
  message: string;
  blueprintStep: BlueprintRouteStep;
  onRetry?: () => void;
  retryLabel?: string;
}

const CloudLoginTransition: React.FC<CloudLoginTransitionProps> = ({
  message,
  blueprintStep,
  onRetry,
  retryLabel,
}) => {
  const { t } = useTranslation();
  const showWindowControls = isDesktopShell() && !isMacOS();

  return (
    <AuthShell
      mode='cloud'
      phase={blueprintStep === 7 ? 'verifying' : 'idle'}
      activationLevel={blueprintStep === 7 ? EMAIL_OTP_LENGTH : 0}
      blueprintStep={blueprintStep}
      brandTitle={t('cloudLogin.brand')}
      brandTagline={t('cloudLogin.brandTagline')}
      brandFlow={t('cloudLogin.brandFlow')}
      brandLogo={<img src={appLogo} alt='' />}
      brandMeta={t('cloudLogin.brandMeta')}
      brandLegal={<CloudLoginLegalLinks />}
      windowControls={
        showWindowControls ? (
          <>
            <div className='flowy-auth-chrome__spacer' data-tauri-drag-region />
            <WindowControls />
          </>
        ) : undefined
      }
      className='cloud-login-page cloud-login-page--transition'
    >
      <div
        className={`cloud-login-transition${onRetry ? ' cloud-login-transition--error' : ''}`}
        role='status'
        aria-live='polite'
        aria-busy={onRetry ? 'false' : 'true'}
      >
        {!onRetry && <span className='flowy-auth-spinner cloud-login-transition__spinner' aria-hidden='true' />}
        <span className='cloud-login-transition__message'>{message}</span>
        {onRetry && retryLabel && (
          <AuthPrimaryButton type='button' onClick={onRetry}>
            {retryLabel}
          </AuthPrimaryButton>
        )}
      </div>
    </AuthShell>
  );
};

/**
 * The auth form owns its hooks in a stable child. CloudLoginPage can therefore
 * switch checking -> authenticated without changing this component's hook list.
 */
const CloudLoginFlow: React.FC<CloudLoginFlowProps> = ({ status, whoami, logout, onSuccess }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [emailTouched, setEmailTouched] = useState(false);
  const [blueprintStep, setBlueprintStep] = useState<BlueprintRouteStep>(0);
  const flow = useEmailOtpLogin({
    autoStart: status === 'unauthenticated',
    onSuccess,
  });

  const showOtp = flow.state.step === 'otp';
  const validEmail = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(flow.email.trim());
  const emailInputEnergy = showOtp || !flow.email.trim()
    ? 0
    : Math.min(1, flow.email.trim().length / 18 + (validEmail ? 0.2 : 0));
  const showWindowControls = isDesktopShell() && !isMacOS();

  useEffect(() => {
    setBlueprintStep(showOtp ? 2 : 0);
  }, [showOtp]);

  useEffect(() => {
    if (!showOtp) return;
    const checkpoint = getOtpBlueprintCheckpoint(flow.code.length);
    setBlueprintStep((2 + checkpoint) as BlueprintRouteStep);
  }, [flow.code.length, showOtp]);

  useEffect(() => {
    if (flow.phase === 'verifying' || flow.phase === 'success') {
      setBlueprintStep(7);
    }
  }, [flow.phase]);

  const handleSendCode = useCallback(() => {
    if (validEmail && !flow.busy && flow.cooldown <= 0) {
      setBlueprintStep((previous) => Math.max(previous, 1) as BlueprintRouteStep);
    }
    void flow.sendCode();
  }, [flow.busy, flow.cooldown, flow.sendCode, validEmail]);

  const handleEmailChange = useCallback((value: string) => {
    flow.setEmail(value);
    if (!value.trim()) setBlueprintStep(0);
  }, [flow.setEmail]);

  const handleChangeEmail = useCallback(() => {
    flow.changeEmail();
    setBlueprintStep(0);
    setEmailTouched(false);
  }, [flow.changeEmail]);

  const handleResendCode = useCallback(() => {
    setBlueprintStep(2);
    void flow.resendCode();
  }, [flow.resendCode]);

  const handleLogout = useCallback(async () => {
    try {
      await logout();
      flow.reset();
      await flow.startLogin();
    } catch {
      // CloudAuth owns the logout error presentation. Auth UI stays stable.
    }
  }, [flow.reset, flow.startLogin, logout]);

  const intentPhase = resolveIntentPhase(
    flow.state.step,
    flow.phase,
    flow.failureKind,
    Boolean(flow.email.trim() || flow.code)
  );
  const activationLevel = (
    flow.phase === 'success'
      ? EMAIL_OTP_LENGTH
      : Math.min(EMAIL_OTP_LENGTH, flow.code.length)
  ) as CloudActivationLevel;
  const statusTone = flow.phase === 'success'
    ? 'success'
    : flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown' || flow.phase === 'session-expired'
      ? 'error'
      : flow.phase === 'transport-error' || flow.failureKind === 'transport' || flow.failureKind === 'verification-pending'
        ? 'warning'
        : 'info';
  const isVerifying = flow.phase === 'verifying';

  const emailError = !showOtp && emailTouched && flow.email.trim() && !validEmail
    ? t('cloudLogin.login.emailInvalid')
    : !showOtp && flow.phase === 'email'
      ? flow.message
      : undefined;
  const statusMessage = flow.message
    ?? (flow.code.length === EMAIL_OTP_LENGTH
      ? t('cloudLogin.login.verifyingWorkspace')
      : t('cloudLogin.login.otpHint'));

  const isSignedIn = Boolean(whoami?.authenticated);
  const blueprintStepOverride = blueprintStep > 0 ? blueprintStep : undefined;

  return (
    <AuthShell
      mode='cloud'
      phase={intentPhase}
      activationLevel={activationLevel}
      inputEnergy={emailInputEnergy}
      blueprintStep={blueprintStepOverride}
      brandTitle={t('cloudLogin.brand')}
      brandTagline={t('cloudLogin.brandTagline')}
      brandFlow={t('cloudLogin.brandFlow')}
      brandLogo={<img src={appLogo} alt='' />}
      brandMeta={t('cloudLogin.brandMeta')}
      brandLegal={<CloudLoginLegalLinks />}
      windowControls={
        showWindowControls ? (
          <>
            <div className='flowy-auth-chrome__spacer' data-tauri-drag-region />
            <WindowControls />
          </>
        ) : undefined
      }
      className='cloud-login-page'
    >
      {isSignedIn ? (
        <section className='cloud-login-account'>
          <h1 className='flowy-auth-heading'>{t('cloudLogin.account.signedIn')}</h1>
          <p className='flowy-auth-description'>{t('cloudLogin.accountSubtitle')}</p>
          {(whoami?.email || whoami?.username) && (
            <p className='cloud-login-account__identity'>{whoami.email ?? whoami.username}</p>
          )}
          <div className='cloud-login-account__actions'>
            <AuthPrimaryButton type='button' onClick={() => navigate('/guid')}>
              {t('cloudLogin.login.continue')}
            </AuthPrimaryButton>
            <button
              type='button'
              className='flowy-auth-secondary'
              disabled={flow.busy}
              onClick={() => void handleLogout()}
            >
              {t('cloudLogin.login.logout')}
            </button>
          </div>
        </section>
      ) : (
        <div className='cloud-login-stage'>
          <div className='flowy-auth-progress' role='list' aria-label={t('cloudLogin.login.progressLabel')}>
            <div className={`flowy-auth-progress__step ${showOtp ? 'is-complete' : 'is-active'}`} role='listitem'>
              <span className='flowy-auth-progress__number'>{showOtp ? '✓' : '01'}</span>
              <span>{t('cloudLogin.login.stepEmail')}</span>
            </div>
            <span className='flowy-auth-progress__line' aria-hidden='true' />
            <div className={`flowy-auth-progress__step ${showOtp ? 'is-active' : ''}`} role='listitem'>
              <span className='flowy-auth-progress__number'>02</span>
              <span>{t('cloudLogin.login.stepOtp')}</span>
            </div>
          </div>

          <div className='cloud-login-copy'>
            <h1 className='flowy-auth-heading'>
              {showOtp ? t('cloudLogin.login.stepOtp') : t('cloudLogin.welcomeTitle')}
            </h1>
            <p className='flowy-auth-description'>
              {showOtp
                ? t('cloudLogin.login.otpSentTo', { email: flow.email.trim() })
                : t('cloudLogin.welcomeDesc')}
            </p>
          </div>

          <form
            className='flowy-auth-form cloud-login-form'
            onSubmit={(event) => {
              event.preventDefault();
              if (showOtp) void flow.verifyCode();
              else handleSendCode();
            }}
          >
            <div className={`cloud-login-input-slot${showOtp ? ' is-otp' : ''}`}>
              {!showOtp ? (
                <AuthField
                  id='cloud-email'
                  type='email'
                  label={t('cloudLogin.login.emailLabel')}
                  placeholder={t('cloudLogin.login.emailPlaceholder')}
                  autoComplete='email'
                  autoFocus
                  value={flow.email}
                  onChange={(event) => handleEmailChange(event.target.value)}
                  onBlur={() => setEmailTouched(true)}
                  disabled={flow.busy}
                  required
                  error={emailError}
                />
              ) : (
                <div className='cloud-login-otp-section'>
                  <div className='cloud-login-otp-heading'>
                    <label className='flowy-auth-label' htmlFor='cloud-otp'>
                      {t('cloudLogin.login.otpLabel')}
                    </label>
                    <button type='button' className='flowy-auth-ghost' disabled={flow.busy} onClick={handleChangeEmail}>
                      {t('cloudLogin.login.changeEmail')}
                    </button>
                  </div>
                  <OtpCodeInput
                    id='cloud-otp'
                    label={t('cloudLogin.login.otpLabel')}
                    value={flow.code}
                    onChange={flow.setCode}
                    onComplete={(code) => void flow.verifyCode(code)}
                    onSubmit={(code) => void flow.verifyCode(code)}
                    autoFocus
                    focusOnErrorReset={flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown'}
                    disabled={flow.busy || flow.phase === 'session-expired' || !flow.pendingId}
                    aria-describedby='cloud-otp-status'
                    aria-required='true'
                  />
                </div>
              )}
            </div>

            <div className='cloud-login-status-slot'>
              {!showOtp ? (
                <AuthCooldownHint seconds={flow.cooldown}>
                  {t('cloudLogin.login.cooldownHint', { seconds: flow.cooldown })}
                </AuthCooldownHint>
              ) : (
                <>
                  <AuthStatusBar
                    id='cloud-otp-status'
                    tone={statusTone}
                    className={isVerifying ? 'cloud-login-status--verifying' : undefined}
                    reserveActionSpace
                    action={
                      flow.recoveryAction === 'retry-verification' && flow.pendingId && flow.code.length === EMAIL_OTP_LENGTH ? (
                        <button type='button' className='flowy-auth-ghost' onClick={() => void flow.verifyCode()}>
                          {flow.failureKind === 'verification-pending'
                            ? t('cloudLogin.login.retryPending')
                            : t('cloudLogin.login.retryVerification')}
                        </button>
                      ) : undefined
                    }
                  >
                    {isVerifying && (
                      <span className='flowy-auth-spinner flowy-auth-status__spinner' aria-hidden='true' />
                    )}
                    <span>{statusMessage}</span>
                  </AuthStatusBar>
                  <AuthCooldownHint seconds={flow.cooldown} showVisual={false} className='flowy-auth-cooldown'>
                    {t('cloudLogin.login.cooldownHint', { seconds: flow.cooldown })}
                  </AuthCooldownHint>
                </>
              )}
            </div>

            <div className='cloud-login-action-slot'>
              {showOtp ? (
                <button
                  type='button'
                  className='flowy-auth-secondary cloud-login-resend'
                  disabled={flow.cooldown > 0 || flow.busy}
                  onClick={handleResendCode}
                >
                  <span>{t('cloudLogin.login.resendCode')}</span>
                  <span className='cloud-login-resend__value'>
                    {flow.cooldown > 0
                      ? t('cloudLogin.login.cooldownButton', { seconds: flow.cooldown })
                      : <Refresh theme='outline' size={16} aria-hidden='true' />}
                  </span>
                </button>
              ) : (
                <AuthPrimaryButton
                  type='submit'
                  loading={flow.busy}
                  loadingLabel={t('cloudLogin.login.sendingCode')}
                  disabled={!validEmail || flow.cooldown > 0}
                  icon={<span>→</span>}
                >
                  {validEmail ? t('cloudLogin.login.sendCodeArrow') : t('cloudLogin.login.emailBeforeSend')}
                </AuthPrimaryButton>
              )}
            </div>
          </form>
        </div>
      )}
    </AuthShell>
  );
};

const CloudLoginPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status, whoami, refresh, logout } = useCloudAuth();
  const justLoggedInRef = useRef(false);
  const navigationRunRef = useRef(0);
  const [isCompleting, setIsCompleting] = useState(false);
  const [isSessionReady, setIsSessionReady] = useState(false);
  const [completionError, setCompletionError] = useState(false);

  const completeSession = useCallback(async () => {
    setIsSessionReady(false);
    try {
      const refreshResult = await refresh({ forceModelSync: true });
      if (refreshResult !== 'authenticated') {
        justLoggedInRef.current = false;
        setCompletionError(true);
      } else {
        setCompletionError(false);
      }
    } catch (error) {
      console.error('Failed to refresh the cloud session after login:', error);
      justLoggedInRef.current = false;
      setCompletionError(true);
    } finally {
      setIsSessionReady(true);
    }
  }, [refresh]);

  const onSuccess = useCallback(async () => {
    if (justLoggedInRef.current) return;
    justLoggedInRef.current = true;
    setIsCompleting(true);
    setCompletionError(false);
    setIsSessionReady(false);
    trackFunnelEvent('auth_completed', { method: 'email_otp' });
    await completeSession();
  }, [completeSession]);

  const retryCompletion = useCallback(() => {
    if (!completionError) return;
    justLoggedInRef.current = true;
    setCompletionError(false);
    void completeSession();
  }, [completeSession, completionError]);

  useEffect(() => {
    document.body.classList.add('login-page-active');
    document.title = t('cloudLogin.pageTitle');
    return () => document.body.classList.remove('login-page-active');
  }, [t]);

  useEffect(() => {
    if (!isCompleting) return;
    if (status === 'unauthenticated') {
      justLoggedInRef.current = false;
      setIsCompleting(false);
      setCompletionError(false);
      setIsSessionReady(false);
      return;
    }
    if (!justLoggedInRef.current || !isSessionReady || status !== 'authenticated') return;

    const navigationRun = ++navigationRunRef.current;
    preloadCommercialPathChunks();
    void preloadGuidPathChunk()
      .catch(() => undefined)
      .then(() => {
        if (navigationRunRef.current !== navigationRun || !justLoggedInRef.current) return;
        justLoggedInRef.current = false;
        navigate('/guid', { replace: true });
      });

    return () => {
      navigationRunRef.current += 1;
    };
  }, [isCompleting, isSessionReady, navigate, status]);

  if (isCompleting || status === 'checking') {
    const isCompletionError = isCompleting && completionError;
    return (
      <CloudLoginTransition
        message={t(
          isCompletionError
            ? 'cloudLogin.login.workspacePreparationFailed'
            : isCompleting
              ? 'cloudLogin.login.successRedirect'
              : 'cloudLogin.login.starting'
        )}
        blueprintStep={isCompleting ? 7 : 0}
        onRetry={isCompletionError ? retryCompletion : undefined}
        retryLabel={isCompletionError ? t('cloudLogin.login.retryWorkspacePreparation') : undefined}
      />
    );
  }

  return (
    <CloudLoginFlow
      status={status}
      whoami={whoami}
      logout={logout}
      onSuccess={onSuccess}
    />
  );
};

export { CloudLoginFlow, resolveIntentPhase };
export default CloudLoginPage;
