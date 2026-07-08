/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate } from 'react-router-dom';
import { Button, Divider, Input, Message, Steps, Switch, Typography } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { ICloudDeviceActivationStatus, ICloudServerSettings, ICloudWhoami } from '@/common/adapter/ipcBridge';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import SettingsPageWrapper from './components/SettingsPageWrapper';

type LoginStep = 'email' | 'otp' | 'done';

const CloudLoginSettings: React.FC = () => {
  const { t } = useTranslation();
  const [developerMode] = useConfig('system.developerMode');
  const [serverSettings, setServerSettings] = useState<ICloudServerSettings | null>(null);
  const [whoami, setWhoami] = useState<ICloudWhoami | null>(null);
  const [deviceStatus, setDeviceStatus] = useState<ICloudDeviceActivationStatus | null>(null);
  const [loginStep, setLoginStep] = useState<LoginStep>('email');
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [loginMessage, setLoginMessage] = useState('');
  const [email, setEmail] = useState('');
  const [otp, setOtp] = useState('');
  const [loading, setLoading] = useState(true);
  const [savingSettings, setSavingSettings] = useState(false);
  const [loginLoading, setLoginLoading] = useState(false);
  const [deviceRetryLoading, setDeviceRetryLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [settings, user] = await Promise.all([
        ipcBridge.cloud.getSettings.invoke(),
        ipcBridge.cloud.whoami.invoke(),
      ]);
      setServerSettings(settings);
      setWhoami(user);
      if (user.authenticated) {
        setLoginStep('done');
        const status = await ipcBridge.cloud.deviceStatus.invoke();
        setDeviceStatus(status);
      } else {
        setDeviceStatus(null);
      }
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

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
      Message.success(t('cloudLogin.settings.saved'));
      void refresh();
    } catch (e) {
      Message.error(String(e));
    } finally {
      setSavingSettings(false);
    }
  };

  const startLogin = async () => {
    setLoginLoading(true);
    try {
      const res = await ipcBridge.cloud.loginStart.invoke({ method: 'email_otp' });
      setPendingId(res.pendingId);
      setLoginMessage(res.message);
      setLoginStep('email');
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoginLoading(false);
    }
  };

  const submitEmail = async () => {
    if (!pendingId || !email.trim()) {
      Message.warning(t('cloudLogin.login.emailRequired'));
      return;
    }
    setLoginLoading(true);
    try {
      const res = await ipcBridge.cloud.loginContinue.invoke({
        pendingId,
        input: { type: 'email', address: email.trim() },
      });
      if (res.status === 'pending') {
        setPendingId(res.pendingId);
        setLoginMessage(res.message);
        setLoginStep('otp');
        Message.info(res.message);
      } else if (res.status === 'success') {
        setLoginStep('done');
        Message.success(t('cloudLogin.login.success'));
        void refresh();
      } else {
        Message.error(res.error);
      }
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoginLoading(false);
    }
  };

  const submitOtp = async () => {
    if (!pendingId || !otp.trim()) {
      Message.warning(t('cloudLogin.login.otpRequired'));
      return;
    }
    setLoginLoading(true);
    try {
      const res = await ipcBridge.cloud.loginContinue.invoke({
        pendingId,
        input: { type: 'otp_code', code: otp.trim() },
      });
      if (res.status === 'success') {
        setLoginStep('done');
        setOtp('');
        Message.success(t('cloudLogin.login.success'));
        void refresh();
      } else if (res.status === 'pending') {
        setPendingId(res.pendingId);
        setLoginMessage(res.message);
        Message.info(res.message);
      } else {
        Message.error(res.error);
        setLoginStep('email');
        setPendingId(null);
      }
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoginLoading(false);
    }
  };

  const logout = async () => {
    setLoginLoading(true);
    try {
      await ipcBridge.cloud.logout.invoke();
      setLoginStep('email');
      setPendingId(null);
      setEmail('');
      setOtp('');
      Message.success(t('cloudLogin.login.loggedOut'));
      void refresh();
    } catch (e) {
      Message.error(String(e));
    } finally {
      setLoginLoading(false);
    }
  };

  const retryDeviceActivation = async () => {
    setDeviceRetryLoading(true);
    try {
      const res = await ipcBridge.cloud.retryDeviceActivation.invoke();
      Message.success(
        res.reported
          ? t('cloudLogin.device.reported')
          : t('cloudLogin.device.upToDate')
      );
      void refresh();
    } catch (e) {
      Message.error(String(e));
    } finally {
      setDeviceRetryLoading(false);
    }
  };

  const stepIndex = loginStep === 'email' ? 1 : loginStep === 'otp' ? 2 : 3;

  if (!developerMode) {
    return <Navigate to='/settings/system' replace />;
  }

  return (
    <SettingsPageWrapper>
      <div className='flex flex-col gap-20px max-w-640px'>
        <div>
          <Typography.Title heading={5} className='!m-0'>
            {t('cloudLogin.title')}
          </Typography.Title>
          <Typography.Paragraph className='!mb-0 text-t-tertiary text-13px'>
            {t('cloudLogin.description')}
          </Typography.Paragraph>
        </div>

        {whoami?.authenticated ? (
          <div className='rd-8px border border-[var(--color-border-2)] p-16px flex flex-col gap-8px'>
            <div className='text-t-primary font-500'>{t('cloudLogin.account.signedIn')}</div>
            {whoami.email && <div className='text-13px text-t-secondary'>{whoami.email}</div>}
            {whoami.username && <div className='text-13px text-t-secondary'>{whoami.username}</div>}
            {whoami.serverBaseUrl && (
              <div className='text-12px text-t-tertiary'>{whoami.serverBaseUrl}</div>
            )}
            {deviceStatus && (
              <div className='mt-8px pt-12px border-t border-[var(--color-border-2)] flex flex-col gap-6px'>
                <div className='text-13px text-t-primary font-500'>{t('cloudLogin.device.title')}</div>
                <div className='text-12px text-t-secondary'>
                  {deviceStatus.activatedForVersion
                    ? t('cloudLogin.device.activated')
                    : t('cloudLogin.device.pending')}
                </div>
                {deviceStatus.serialNumber && (
                  <div className='text-12px text-t-tertiary'>
                    {t('cloudLogin.device.serial')}: {deviceStatus.serialNumber}
                  </div>
                )}
                {deviceStatus.appVersion && (
                  <div className='text-12px text-t-tertiary'>
                    {t('cloudLogin.device.version')}: {deviceStatus.appVersion}
                  </div>
                )}
                {deviceStatus.lastReportedIp && (
                  <div className='text-12px text-t-tertiary'>
                    {t('cloudLogin.device.ip')}: {deviceStatus.lastReportedIp}
                  </div>
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
          <div className='flex flex-col gap-16px'>
            <Steps current={stepIndex} size='small'>
              <Steps.Step title={t('cloudLogin.login.stepStart')} />
              <Steps.Step title={t('cloudLogin.login.stepEmail')} />
              <Steps.Step title={t('cloudLogin.login.stepOtp')} />
            </Steps>

            {!pendingId ? (
              <Button type='primary' loading={loginLoading || loading} onClick={startLogin}>
                {t('cloudLogin.login.start')}
              </Button>
            ) : (
              <div className='flex flex-col gap-12px'>
                {loginMessage && <div className='text-12px text-t-tertiary'>{loginMessage}</div>}

                {loginStep === 'email' && (
                  <>
                    <Input
                      value={email}
                      onChange={setEmail}
                      placeholder={t('cloudLogin.login.emailPlaceholder')}
                      onPressEnter={submitEmail}
                    />
                    <Button type='primary' loading={loginLoading} onClick={submitEmail}>
                      {t('cloudLogin.login.sendCode')}
                    </Button>
                  </>
                )}

                {loginStep === 'otp' && (
                  <>
                    <Input
                      value={otp}
                      onChange={setOtp}
                      placeholder={t('cloudLogin.login.otpPlaceholder')}
                      maxLength={8}
                      onPressEnter={submitOtp}
                    />
                    <div className='flex gap-8px'>
                      <Button type='primary' loading={loginLoading} onClick={submitOtp}>
                        {t('cloudLogin.login.verify')}
                      </Button>
                      <Button onClick={() => setLoginStep('email')}>{t('cloudLogin.login.back')}</Button>
                    </div>
                  </>
                )}
              </div>
            )}
          </div>
        )}

        <Divider />

        {serverSettings && (
          <div className='flex flex-col gap-14px'>
            <Typography.Title heading={6} className='!m-0'>
              {t('cloudLogin.settings.title')}
            </Typography.Title>

            <div className='flex items-center justify-between'>
              <span className='text-t-primary text-14px font-500'>{t('cloudLogin.settings.enabled')}</span>
              <Switch
                checked={serverSettings.enabled}
                onChange={(v) => setServerSettings({ ...serverSettings, enabled: v })}
              />
            </div>

            <div className='flex flex-col gap-6px'>
              <span className='text-t-secondary text-13px'>{t('cloudLogin.settings.baseUrl')}</span>
              <Input
                value={serverSettings.baseUrl}
                onChange={(v) => setServerSettings({ ...serverSettings, baseUrl: v })}
              />
            </div>

            <div className='grid grid-cols-1 md:grid-cols-2 gap-12px'>
              <div className='flex flex-col gap-6px'>
                <span className='text-t-secondary text-13px'>{t('cloudLogin.settings.channel')}</span>
                <Input
                  value={serverSettings.channel}
                  onChange={(v) => setServerSettings({ ...serverSettings, channel: v })}
                />
              </div>
              <div className='flex flex-col gap-6px'>
                <span className='text-t-secondary text-13px'>{t('cloudLogin.settings.app')}</span>
                <Input value={serverSettings.app} onChange={(v) => setServerSettings({ ...serverSettings, app: v })} />
              </div>
            </div>

            <Button type='primary' loading={savingSettings || loading} onClick={saveServerSettings}>
              {t('common.save', { defaultValue: 'Save' })}
            </Button>
          </div>
        )}
      </div>
    </SettingsPageWrapper>
  );
};

export default CloudLoginSettings;
