import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate } from 'react-router-dom';
import { Button, Input, Message, Steps, Switch } from '@arco-design/web-react';
import { Refresh } from '@icon-park/react';
import { ipcBridge } from '@/common';
import type { ICloudDeviceActivationStatus, ICloudServerSettings, ICloudWhoami } from '@/common/adapter/ipcBridge';
import AuthField from '@renderer/components/auth/AuthField';
import AuthCooldownHint from '@renderer/components/auth/AuthCooldownHint';
import AuthPrimaryButton from '@renderer/components/auth/AuthPrimaryButton';
import AuthStatusBar from '@renderer/components/auth/AuthStatusBar';
import OtpCodeInput from '@renderer/components/auth/OtpCodeInput';
import useEmailOtpLogin from '@renderer/hooks/auth/useEmailOtpLogin';
import {
  SettingsActionBar,
  SettingsList,
  SettingsPageHeader,
  SettingsSection,
  SettingsRow,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import SettingsPageWrapper from './components/SettingsPageWrapper';
import './CloudLoginSettings.css';
import '@renderer/components/auth/auth.css';

const CloudLoginSettings: React.FC = () => {
  const { t } = useTranslation();
  const [developerMode] = useConfig('system.developerMode');
  const [serverSettings, setServerSettings] = useState<ICloudServerSettings | null>(null);
  const [savedServerSettings, setSavedServerSettings] = useState<ICloudServerSettings | null>(null);
  const [serverSettingsError, setServerSettingsError] = useState<string | null>(null);
  const [whoami, setWhoami] = useState<ICloudWhoami | null>(null);
  const [deviceStatus, setDeviceStatus] = useState<ICloudDeviceActivationStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [savingSettings, setSavingSettings] = useState(false);
  const [loginLoading, setLoginLoading] = useState(false);
  const [deviceRetryLoading, setDeviceRetryLoading] = useState(false);
  const [emailTouched, setEmailTouched] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [settings, user] = await Promise.all([
        ipcBridge.cloud.getSettings.invoke(),
        ipcBridge.cloud.whoami.invoke(),
      ]);
      setServerSettings(settings);
      setSavedServerSettings(settings);
      setWhoami(user);
      if (user.authenticated) {
        const status = await ipcBridge.cloud.deviceStatus.invoke();
        setDeviceStatus(status);
      } else {
        setDeviceStatus(null);
      }
    } catch (error) {
      Message.error(String(error));
    } finally {
      setLoading(false);
    }
  }, []);

  const flow = useEmailOtpLogin({
    onSuccess: refresh,
  });

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveServerSettings = async () => {
    if (!serverSettings) return;
    setSavingSettings(true);
    try {
      const saved = await ipcBridge.cloud.updateSettings.invoke({
        enabled: serverSettings.enabled,
        baseUrl: serverSettings.baseUrl,
        channel: serverSettings.channel,
        app: serverSettings.app,
      });
      setServerSettings(saved);
      setSavedServerSettings(saved);
      setServerSettingsError(null);
      void refresh();
    } catch (error) {
      setServerSettingsError(String(error));
    } finally {
      setSavingSettings(false);
    }
  };

  const logout = async () => {
    setLoginLoading(true);
    try {
      await ipcBridge.cloud.logout.invoke();
      flow.reset();
      Message.success(t('cloudLogin.login.loggedOut'));
      void refresh();
    } catch (error) {
      Message.error(String(error));
    } finally {
      setLoginLoading(false);
    }
  };

  const retryDeviceActivation = async () => {
    setDeviceRetryLoading(true);
    try {
      const response = await ipcBridge.cloud.retryDeviceActivation.invoke();
      Message.success(response.reported ? t('cloudLogin.device.reported') : t('cloudLogin.device.upToDate'));
      void refresh();
    } catch (error) {
      Message.error(String(error));
    } finally {
      setDeviceRetryLoading(false);
    }
  };

  const stepIndex = whoami?.authenticated ? 3 : flow.state.step === 'otp' ? 2 : flow.hasSession ? 1 : 0;
  const hasServerSettingsChanges = Boolean(
    serverSettings &&
      savedServerSettings &&
      JSON.stringify(serverSettings) !== JSON.stringify(savedServerSettings)
  );
  const showOtp = flow.state.step === 'otp';
  const statusTone = flow.phase === 'session-expired'
    ? 'error'
    : flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown'
      ? 'error'
      : flow.phase === 'transport-error' || flow.failureKind === 'transport' || flow.failureKind === 'verification-pending'
        ? 'warning'
        : 'info';
  const otpStatus = flow.message
    ?? (flow.code.length === 6 ? t('cloudLogin.login.verifyingWorkspace') : t('cloudLogin.login.otpHint'));

  if (!developerMode) return <Navigate to='/settings/system' replace />;

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader title={t('cloudLogin.title')} description={t('cloudLogin.description')} />

        {whoami?.authenticated ? (
          <div className='flowy-settings-panel p-16px flex flex-col gap-8px'>
            <div className='text-t-primary font-500'>{t('cloudLogin.account.signedIn')}</div>
            {whoami.email && <div className='text-13px text-t-secondary'>{whoami.email}</div>}
            {whoami.username && <div className='text-13px text-t-secondary'>{whoami.username}</div>}
            {whoami.serverBaseUrl && <div className='text-12px text-t-tertiary'>{whoami.serverBaseUrl}</div>}
            {deviceStatus && (
              <div className='mt-8px pt-12px border-t border-[var(--color-border-2)] flex flex-col gap-6px'>
                <div className='text-13px text-t-primary font-500'>{t('cloudLogin.device.title')}</div>
                <div className='text-12px text-t-secondary'>
                  {deviceStatus.activatedForVersion ? t('cloudLogin.device.activated') : t('cloudLogin.device.pending')}
                </div>
                {deviceStatus.serialNumber && (
                  <div className='text-12px text-t-tertiary'>{t('cloudLogin.device.serial')}: {deviceStatus.serialNumber}</div>
                )}
                {deviceStatus.appVersion && (
                  <div className='text-12px text-t-tertiary'>{t('cloudLogin.device.version')}: {deviceStatus.appVersion}</div>
                )}
                {deviceStatus.lastReportedIp && (
                  <div className='text-12px text-t-tertiary'>{t('cloudLogin.device.ip')}: {deviceStatus.lastReportedIp}</div>
                )}
                {!deviceStatus.activatedForVersion && (
                  <Button size='small' loading={deviceRetryLoading} onClick={retryDeviceActivation}>
                    {t('cloudLogin.device.retry')}
                  </Button>
                )}
              </div>
            )}
            <div>
              <Button status='danger' loading={loginLoading} onClick={logout}>
                {t('cloudLogin.login.logout')}
              </Button>
            </div>
          </div>
        ) : (
          <div className='flowy-settings-auth'>
            <Steps current={stepIndex} size='small'>
              <Steps.Step title={t('cloudLogin.login.stepStart')} />
              <Steps.Step title={t('cloudLogin.login.stepEmail')} />
              <Steps.Step title={t('cloudLogin.login.stepOtp')} />
            </Steps>

            {!flow.hasSession && !showOtp ? (
              <div className='flowy-settings-auth__start'>
                {flow.message && <AuthStatusBar tone='error'>{flow.message}</AuthStatusBar>}
                <AuthPrimaryButton
                  type='button'
                  loading={flow.busy}
                  loadingLabel={t('cloudLogin.login.starting')}
                  onClick={() => void flow.startLogin()}
                >
                  {t('cloudLogin.login.start')}
                </AuthPrimaryButton>
              </div>
            ) : showOtp ? (
              <form
                className='flowy-settings-auth__form'
                onSubmit={(event) => {
                  event.preventDefault();
                  void flow.verifyCode();
                }}
              >
                <div className='flowy-settings-auth__input-slot'>
                  <div className='flowy-settings-auth__otp-heading'>
                    <div>
                      <div className='flowy-settings-auth__label'>{t('cloudLogin.login.otpLabel')}</div>
                      <div className='flowy-settings-auth__hint'>{t('cloudLogin.login.otpSentTo', { email: flow.email.trim() })}</div>
                    </div>
                    <button type='button' className='flowy-auth-ghost' disabled={flow.busy} onClick={flow.changeEmail}>
                      {t('cloudLogin.login.changeEmail')}
                    </button>
                  </div>
                  <OtpCodeInput
                    compact
                    id='settings-cloud-otp'
                    label={t('cloudLogin.login.otpLabel')}
                    value={flow.code}
                    onChange={flow.setCode}
                    onComplete={(code) => void flow.verifyCode(code)}
                    onSubmit={(code) => void flow.verifyCode(code)}
                    focusOnErrorReset={flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown'}
                    disabled={flow.busy || flow.phase === 'session-expired' || !flow.pendingId}
                    aria-describedby='settings-cloud-otp-status'
                    aria-required='true'
                  />
                </div>
                <div className='flowy-settings-auth__status-slot'>
                  <AuthStatusBar
                    id='settings-cloud-otp-status'
                    tone={statusTone}
                    reserveActionSpace
                    action={
                      flow.recoveryAction === 'retry-verification' && flow.pendingId && flow.code.length === 6 ? (
                        <button type='button' className='flowy-auth-ghost' onClick={() => void flow.verifyCode()}>
                          {flow.failureKind === 'verification-pending'
                            ? t('cloudLogin.login.retryPending')
                            : t('cloudLogin.login.retryVerification')}
                        </button>
                      ) : undefined
                    }
                  >
                    {otpStatus}
                  </AuthStatusBar>
                  <AuthCooldownHint seconds={flow.cooldown} showVisual={false} className='flowy-auth-cooldown'>
                    {t('cloudLogin.login.cooldownHint', { seconds: flow.cooldown })}
                  </AuthCooldownHint>
                </div>
                <div className='flowy-settings-auth__action-slot'>
                  <button
                    type='button'
                    className='flowy-auth-secondary'
                    disabled={flow.cooldown > 0 || flow.busy}
                    onClick={() => void flow.resendCode()}
                  >
                    <span>{t('cloudLogin.login.resendCode')}</span>
                    <span className='flowy-settings-auth__resend-value'>
                      {flow.cooldown > 0
                        ? t('cloudLogin.login.cooldownButton', { seconds: flow.cooldown })
                        : <Refresh theme='outline' size={16} aria-hidden='true' />}
                    </span>
                  </button>
                </div>
              </form>
            ) : (
              <form
                className='flowy-settings-auth__form'
                onSubmit={(event) => {
                  event.preventDefault();
                  void flow.sendCode();
                }}
              >
                <div className='flowy-settings-auth__input-slot'>
                  <AuthField
                    id='settings-cloud-email'
                    type='email'
                    label={t('cloudLogin.login.emailLabel')}
                    placeholder={t('cloudLogin.login.emailPlaceholder')}
                    autoComplete='email'
                    autoFocus
                    value={flow.email}
                    onChange={(event) => flow.setEmail(event.target.value)}
                    onBlur={() => setEmailTouched(true)}
                    disabled={flow.busy}
                    error={
                      emailTouched && flow.email.trim() && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(flow.email.trim())
                        ? t('cloudLogin.login.emailInvalid')
                        : flow.message ?? undefined
                    }
                    required
                  />
                </div>
                <div className='flowy-settings-auth__status-slot'>
                  <AuthCooldownHint seconds={flow.cooldown}>
                    {t('cloudLogin.login.cooldownHint', { seconds: flow.cooldown })}
                  </AuthCooldownHint>
                </div>
                <div className='flowy-settings-auth__action-slot'>
                  <AuthPrimaryButton
                    type='submit'
                    loading={flow.busy}
                    loadingLabel={t('cloudLogin.login.sendingCode')}
                    icon={<span>→</span>}
                    disabled={flow.cooldown > 0}
                  >
                    {t('cloudLogin.login.sendCodeArrow')}
                  </AuthPrimaryButton>
                </div>
              </form>
            )}
          </div>
        )}

        {serverSettings && (
          <SettingsSection title={t('cloudLogin.settings.title')}>
            <SettingsList>
              <SettingsRow
                label={t('cloudLogin.settings.enabled')}
                control={<Switch checked={serverSettings.enabled} onChange={(value) => setServerSettings({ ...serverSettings, enabled: value })} />}
              />
              <SettingsRow
                label={t('cloudLogin.settings.baseUrl')}
                control={<Input value={serverSettings.baseUrl} onChange={(value) => setServerSettings({ ...serverSettings, baseUrl: value })} />}
              />
              <SettingsRow
                label={t('cloudLogin.settings.channel')}
                control={<Input value={serverSettings.channel} onChange={(value) => setServerSettings({ ...serverSettings, channel: value })} />}
              />
              <SettingsRow
                label={t('cloudLogin.settings.app')}
                control={<Input value={serverSettings.app} onChange={(value) => setServerSettings({ ...serverSettings, app: value })} />}
              />
            </SettingsList>
            <SettingsActionBar
              visible={hasServerSettingsChanges || Boolean(serverSettingsError)}
              saveLabel={t('common.save', { defaultValue: 'Save' })}
              onSave={() => void saveServerSettings()}
              resetLabel={t('common.cancel', { defaultValue: 'Cancel' })}
              onReset={() => {
                setServerSettings(savedServerSettings);
                setServerSettingsError(null);
              }}
              loading={savingSettings || loading}
              error={serverSettingsError}
            />
          </SettingsSection>
        )}
      </div>
    </SettingsPageWrapper>
  );
};

export default CloudLoginSettings;
