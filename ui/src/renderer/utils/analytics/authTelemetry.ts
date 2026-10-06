/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { tauriUpdateCurrentVersion } from '@/common/adapter/tauriUpdater';
import { trackFunnelEvent, type FunnelEvent } from './productFunnel';

export type OtpSendFailureKind =
  | 'invalid-email'
  | 'invalid-code'
  | 'verification-pending'
  | 'verification-failed'
  | 'transport'
  | 'session-expired'
  | 'terminal'
  | 'unexpected'
  | 'unknown';

async function appVersionProp(): Promise<string | null> {
  try {
    const version = (await tauriUpdateCurrentVersion()).trim();
    return version || null;
  } catch {
    return null;
  }
}

async function withVersion(
  props?: FunnelEvent['props']
): Promise<FunnelEvent['props']> {
  const appVersion = await appVersionProp();
  return {
    method: 'email_otp',
    ...(appVersion ? { app_version: appVersion } : {}),
    ...props,
  };
}

export async function trackOtpSendStarted(): Promise<FunnelEvent> {
  return trackFunnelEvent('otp_send_started', await withVersion());
}

export async function trackOtpSendSucceeded(): Promise<FunnelEvent> {
  return trackFunnelEvent('otp_send_succeeded', await withVersion());
}

export async function trackOtpSendFailed(
  failureKind: OtpSendFailureKind
): Promise<FunnelEvent> {
  return trackFunnelEvent(
    'otp_send_failed',
    await withVersion({ failure_kind: failureKind })
  );
}
