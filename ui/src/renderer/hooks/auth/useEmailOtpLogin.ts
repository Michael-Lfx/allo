/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import type { ICloudLoginContinueResponse } from '@/common/adapter/ipcBridge';

export const EMAIL_OTP_LENGTH = 6;
export const EMAIL_OTP_COOLDOWN_SECONDS = 60;

export type EmailOtpPhase =
  | 'email'
  | 'sending'
  | 'otp'
  | 'verifying'
  | 'transport-error'
  | 'session-expired'
  | 'success';

export type OtpFailureKind =
  | 'invalid-code'
  | 'verification-pending'
  | 'verification-failed'
  | 'transport'
  | 'session-expired'
  | 'unknown';

export type OtpRecoveryAction = 'retry-verification' | 'resend-code' | null;

export interface EmailOtpState {
  step: 'email' | 'otp';
  email: string;
  code: string;
  pendingId: string | null;
  phase: EmailOtpPhase;
  failureKind: OtpFailureKind | null;
  cooldown: number;
  message: string | null;
  requestGeneration: number;
}

export interface UseEmailOtpLoginOptions {
  autoStart?: boolean;
  onSuccess?: () => Promise<void> | void;
}

const INITIAL_STATE: EmailOtpState = {
  step: 'email',
  email: '',
  code: '',
  pendingId: null,
  phase: 'email',
  failureKind: null,
  cooldown: 0,
  message: null,
  requestGeneration: 0,
};

export const normalizeOtpCode = (value: string) => value.replace(/\D/g, '').slice(0, EMAIL_OTP_LENGTH);

type ErrorRecord = Record<string, unknown>;

const asErrorRecord = (error: unknown): ErrorRecord | null => (
  error && typeof error === 'object' ? error as ErrorRecord : null
);

const readString = (value: unknown) => typeof value === 'string' ? value : '';

const getErrorStatus = (error: unknown): number | null => {
  const value = asErrorRecord(error)?.status;
  return typeof value === 'number' ? value : null;
};

const getErrorCode = (error: unknown): string => {
  const record = asErrorRecord(error);
  const body = asErrorRecord(record?.body);
  return readString(record?.code) || readString(body?.code);
};

const stringifyErrorValue = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (value === undefined || value === null) return '';
  try {
    return JSON.stringify(value);
  } catch {
    return '';
  }
};

const getErrorText = (error: unknown): string => {
  const record = asErrorRecord(error);
  if (record) {
    const body = asErrorRecord(record.body);
    return [
      readString(record.backendMessage),
      readString(record.code),
      readString(record.message),
      readString(body?.error),
      readString(body?.code),
      readString(body?.message),
      stringifyErrorValue(record.body),
    ].filter(Boolean).join(' ').toLowerCase();
  }
  if (error instanceof Error) return error.message.toLowerCase();
  return stringifyErrorValue(error).toLowerCase();
};

/** Classifies verification failures without exposing backend or upstream text to users. */
export const classifyOtpVerificationError = (error: unknown): OtpFailureKind => {
  const text = getErrorText(error);
  const status = getErrorStatus(error);
  const code = getErrorCode(error).toUpperCase();

  if (code === 'CLOUD_OTP_INVALID_CODE') return 'invalid-code';
  if (
    status === 410
    || status === 429
    || /expired|too\s*many|attempts|session\s+expired|过期|次数|会话失效/i.test(text)
  ) {
    return 'session-expired';
  }
  if (
    /api\s*error\s*:?\s*4\d{2}/i.test(text)
    || status === 400
    || status === 422
    || /invalid[_ -]?(otp|code|verification)|incorrect\s+(otp|code|verification)|wrong\s+(otp|code|verification)|验证码错误|验证码无效|验证码不正确/i.test(text)
  ) {
    return 'invalid-code';
  }
  if (
    [502, 504].includes(status ?? 0)
    || /timeout|timed\s*out|gateway|network|connection|fetch|offline|abort|load\s+failed/i.test(text)
    || /timeout|gateway|unavailable/i.test(code)
  ) {
    return 'transport';
  }
  return 'unknown';
};

export const getOtpRecoveryAction = (failureKind: OtpFailureKind | null): OtpRecoveryAction => {
  if (failureKind === 'transport' || failureKind === 'verification-pending') return 'retry-verification';
  if (failureKind === 'session-expired' || failureKind === 'verification-failed') return 'resend-code';
  return null;
};

const getVerificationFailureMessage = (
  failureKind: OtpFailureKind,
  t: ReturnType<typeof useTranslation>['t']
) => failureKind === 'invalid-code'
  ? t('cloudLogin.errors.invalidCode')
  : failureKind === 'session-expired'
    ? t('cloudLogin.errors.sessionExpired')
    : t('cloudLogin.errors.verificationUnavailable');

/**
 * A JSON `failed` response is terminal: the server has already discarded the
 * pending session. Keep it separate from an invalid code, whose session can
 * still accept another six-digit attempt.
 */
export const isTerminalLoginFailureResponse = (
  response: ICloudLoginContinueResponse
): response is Extract<ICloudLoginContinueResponse, { status: 'failed' }> => response.status === 'failed';

const isEmail = (value: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value);

export const useEmailOtpLogin = ({ autoStart = false, onSuccess }: UseEmailOtpLoginOptions = {}) => {
  const { t } = useTranslation();
  const [state, setState] = useState<EmailOtpState>(INITIAL_STATE);
  const generationRef = useRef(0);
  const autoStartRef = useRef(false);
  const onSuccessRef = useRef(onSuccess);
  onSuccessRef.current = onSuccess;

  const nextGeneration = useCallback(() => {
    generationRef.current += 1;
    return generationRef.current;
  }, []);

  const isCurrent = useCallback((generation: number) => generationRef.current === generation, []);

  const updateState = useCallback((generation: number, update: (previous: EmailOtpState) => EmailOtpState) => {
    if (!isCurrent(generation)) return false;
    setState(update);
    return true;
  }, [isCurrent]);

  const requestSession = useCallback(async (generation: number, preserveOtp = false) => {
    try {
      const response = await ipcBridge.cloud.loginStart.invoke({ method: 'email_otp' });
      if (!isCurrent(generation)) return null;
      setState((previous) => ({ ...previous, pendingId: response.pendingId }));
      return response.pendingId;
    } catch (error) {
      const failureKind = preserveOtp ? classifyOtpVerificationError(error) : null;
      updateState(generation, (previous) => ({
        ...previous,
        step: preserveOtp ? 'otp' : 'email',
        phase: preserveOtp && failureKind === 'transport'
          ? 'transport-error'
          : preserveOtp
            ? 'session-expired'
            : 'email',
        failureKind,
        message: preserveOtp
          ? failureKind === 'transport'
            ? t('cloudLogin.errors.network')
            : t('cloudLogin.errors.verificationUnavailable')
          : t('cloudLogin.errors.unknown'),
      }));
      return null;
    }
  }, [isCurrent, t, updateState]);

  const startLogin = useCallback(async () => {
    const generation = nextGeneration();
    setState((previous) => ({
      ...previous,
      code: '',
      pendingId: null,
      cooldown: 0,
      step: 'email',
      phase: 'sending',
      failureKind: null,
      message: null,
      requestGeneration: generation,
    }));
    const pendingId = await requestSession(generation);
    if (!pendingId || !isCurrent(generation)) return false;
    setState((previous) => ({ ...previous, pendingId, step: 'email', phase: 'email', failureKind: null, message: null }));
    return true;
  }, [isCurrent, nextGeneration, requestSession]);

  const sendCode = useCallback(async () => {
    const email = state.email.trim();
    if (!email || !isEmail(email)) {
      setState((previous) => ({ ...previous, message: t('cloudLogin.login.emailInvalid') }));
      return false;
    }
    if (state.cooldown > 0 || state.phase === 'sending' || state.phase === 'verifying') return false;

    const generation = nextGeneration();
    setState((previous) => ({
      ...previous,
      phase: 'sending',
      failureKind: null,
      message: null,
      code: '',
      requestGeneration: generation,
    }));

    const pendingId = state.pendingId ?? await requestSession(generation);
    if (!pendingId || !isCurrent(generation)) return false;

    try {
      const response = await ipcBridge.cloud.loginContinue.invoke({
        pendingId,
        input: { type: 'email', address: email },
      });
      if (!isCurrent(generation)) return false;
      if (response.status === 'pending') {
        setState((previous) => ({
          ...previous,
          pendingId: response.pendingId,
          step: 'otp',
          phase: 'otp',
          failureKind: null,
          code: '',
          cooldown: EMAIL_OTP_COOLDOWN_SECONDS,
          message: t('cloudLogin.login.codeSent'),
        }));
        return true;
      }
      if (isTerminalLoginFailureResponse(response)) {
        setState((previous) => ({
          ...previous,
          pendingId: null,
          code: '',
          cooldown: 0,
          step: 'email',
          phase: 'email',
          failureKind: 'unknown',
          message: t('cloudLogin.errors.unknown'),
        }));
        return false;
      }
      if (response.status === 'success') {
        setState((previous) => ({ ...previous, phase: 'success', failureKind: null, message: t('cloudLogin.login.successRedirect') }));
        await onSuccessRef.current?.();
        return true;
      }
      setState((previous) => ({ ...previous, phase: 'email', failureKind: 'unknown', message: t('cloudLogin.errors.unknown') }));
      return false;
    } catch {
      updateState(generation, (previous) => ({
        ...previous,
        phase: 'email',
        message: t('cloudLogin.errors.network'),
      }));
      return false;
    }
  }, [isCurrent, nextGeneration, requestSession, state.cooldown, state.email, state.pendingId, state.phase, t, updateState]);

  const verifyCode = useCallback(async (codeOverride?: string) => {
    const code = normalizeOtpCode(codeOverride ?? state.code);
    if (code.length !== EMAIL_OTP_LENGTH) {
      setState((previous) => ({ ...previous, message: t('cloudLogin.login.otpRequired') }));
      return false;
    }
    if (!state.pendingId || state.phase === 'session-expired' || state.phase === 'verifying') {
      if (!state.pendingId) setState((previous) => ({ ...previous, message: t('cloudLogin.login.sendCodeFirst') }));
      return false;
    }

    const generation = nextGeneration();
    setState((previous) => ({
      ...previous,
      code,
      phase: 'verifying',
      failureKind: null,
      message: t('cloudLogin.login.verifyingWorkspace'),
      requestGeneration: generation,
    }));

    try {
      const response = await ipcBridge.cloud.loginContinue.invoke({
        pendingId: state.pendingId,
        input: { type: 'otp_code', code },
      });
      if (!isCurrent(generation)) return false;
      if (response.status === 'success') {
        setState((previous) => ({ ...previous, code: '', phase: 'success', failureKind: null, message: t('cloudLogin.login.successRedirect') }));
        await onSuccessRef.current?.();
        return true;
      }
      if (response.status === 'pending') {
        setState((previous) => ({
          ...previous,
          pendingId: response.pendingId,
          phase: 'transport-error',
          failureKind: 'verification-pending',
          message: t('cloudLogin.login.verificationPending'),
        }));
        return false;
      }
      if (isTerminalLoginFailureResponse(response)) {
        setState((previous) => ({
          ...previous,
          pendingId: null,
          step: 'otp',
          code: '',
          cooldown: 0,
          phase: 'session-expired',
          failureKind: 'verification-failed',
          message: t('cloudLogin.errors.verificationUnavailable'),
        }));
        return false;
      }
      setState((previous) => ({
        ...previous,
        step: 'otp',
        code: '',
        phase: 'otp',
        failureKind: 'invalid-code',
        message: t('cloudLogin.errors.invalidCode'),
      }));
      return false;
    } catch (error) {
      const failureKind = classifyOtpVerificationError(error);
      updateState(generation, (previous) => failureKind === 'transport'
        ? {
            ...previous,
            phase: 'transport-error',
            failureKind,
            message: t('cloudLogin.errors.network'),
          }
        : failureKind === 'session-expired'
          ? {
              ...previous,
              pendingId: null,
              step: 'otp',
              code: '',
              phase: 'session-expired',
              failureKind,
              message: t('cloudLogin.errors.sessionExpired'),
            }
          : {
              ...previous,
              step: 'otp',
              code: '',
              phase: 'otp',
              failureKind,
              message: getVerificationFailureMessage(failureKind, t),
            });
      return false;
    }
  }, [isCurrent, nextGeneration, onSuccessRef, state.code, state.pendingId, state.phase, t, updateState]);

  const setEmail = useCallback((email: string) => {
    setState((previous) => ({
      ...previous,
      email,
      failureKind: previous.phase === 'email' ? null : previous.failureKind,
      message: previous.phase === 'email' ? null : previous.message,
    }));
  }, []);

  const setCode = useCallback((code: string) => {
    const normalized = normalizeOtpCode(code);
    setState((previous) => ({
      ...previous,
      code: normalized,
      phase: previous.phase === 'transport-error' ? 'otp' : previous.phase,
      failureKind: previous.phase === 'transport-error' || previous.phase === 'otp' ? null : previous.failureKind,
      message: previous.phase === 'otp' || previous.phase === 'transport-error' ? null : previous.message,
    }));
  }, []);

  const changeEmail = useCallback(() => {
    const generation = nextGeneration();
    setState((previous) => ({
      ...previous,
      pendingId: null,
      code: '',
      cooldown: 0,
      step: 'email',
      phase: 'email',
      failureKind: null,
      message: null,
      requestGeneration: generation,
    }));
  }, [nextGeneration]);

  const resendCode = useCallback(async () => {
    if (state.cooldown > 0 || state.phase === 'sending' || state.phase === 'verifying') return false;
    const email = state.email.trim();
    if (!email || !isEmail(email)) {
      setState((previous) => ({ ...previous, phase: 'email', message: t('cloudLogin.login.emailInvalid') }));
      return false;
    }

    const generation = nextGeneration();
    setState((previous) => ({
      ...previous,
      pendingId: null,
      code: '',
      step: 'otp',
      phase: 'sending',
      failureKind: null,
      message: null,
      requestGeneration: generation,
    }));
    const newPendingId = await requestSession(generation, true);
    if (!newPendingId || !isCurrent(generation)) return false;

    try {
      const response = await ipcBridge.cloud.loginContinue.invoke({
        pendingId: newPendingId,
        input: { type: 'email', address: email },
      });
      if (!isCurrent(generation)) return false;
      if (response.status === 'pending') {
        setState((previous) => ({
          ...previous,
          pendingId: response.pendingId,
          step: 'otp',
          code: '',
          phase: 'otp',
          failureKind: null,
          cooldown: EMAIL_OTP_COOLDOWN_SECONDS,
          message: t('cloudLogin.login.codeSent'),
        }));
        return true;
      }
      if (isTerminalLoginFailureResponse(response)) {
        setState((previous) => ({
          ...previous,
          pendingId: null,
          code: '',
          cooldown: 0,
          step: 'email',
          phase: 'email',
          failureKind: 'unknown',
          message: t('cloudLogin.errors.unknown'),
        }));
        return false;
      }
      if (response.status === 'success') {
        setState((previous) => ({ ...previous, phase: 'success', failureKind: null, message: t('cloudLogin.login.successRedirect') }));
        await onSuccessRef.current?.();
        return true;
      }
      setState((previous) => ({ ...previous, phase: 'email', failureKind: 'unknown', message: t('cloudLogin.errors.unknown') }));
      return false;
    } catch (error) {
      const failureKind = classifyOtpVerificationError(error);
      updateState(generation, (previous) => failureKind === 'transport'
        ? {
            ...previous,
            phase: 'transport-error',
            failureKind,
            message: t('cloudLogin.errors.network'),
          }
        : {
            ...previous,
            pendingId: null,
            step: 'otp',
            phase: 'session-expired',
            failureKind,
            message: getVerificationFailureMessage(failureKind, t),
          });
      return false;
    }
  }, [isCurrent, nextGeneration, requestSession, state.cooldown, state.email, state.phase, t, updateState]);

  const reset = useCallback(() => {
    const generation = nextGeneration();
    setState({ ...INITIAL_STATE, requestGeneration: generation });
  }, [nextGeneration]);

  useEffect(() => {
    if (!autoStart || autoStartRef.current) return;
    autoStartRef.current = true;
    void startLogin();
  }, [autoStart, startLogin]);

  useEffect(() => {
    if (state.cooldown <= 0) return undefined;
    const timer = window.setTimeout(() => {
      setState((previous) => ({ ...previous, cooldown: Math.max(0, previous.cooldown - 1) }));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [state.cooldown]);

  return {
    state,
    email: state.email,
    code: state.code,
    pendingId: state.pendingId,
    phase: state.phase,
    failureKind: state.failureKind,
    recoveryAction: getOtpRecoveryAction(state.failureKind),
    cooldown: state.cooldown,
    message: state.message,
    hasSession: Boolean(state.pendingId),
    busy: state.phase === 'sending' || state.phase === 'verifying',
    setEmail,
    setCode,
    startLogin,
    sendCode,
    verifyCode,
    resendCode,
    changeEmail,
    reset,
  };
};

export default useEmailOtpLogin;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
