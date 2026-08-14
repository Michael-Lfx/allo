/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  classifyOtpVerificationError,
  EMAIL_OTP_COOLDOWN_SECONDS,
  EMAIL_OTP_LENGTH,
  getOtpRecoveryAction,
  isTerminalLoginFailureResponse,
  normalizeOtpCode,
} from '@renderer/hooks/auth/useEmailOtpLogin';
import {
  BLUEPRINT_AMBIENT_MARKS,
  BLUEPRINT_FRAGMENTS,
  BLUEPRINT_ROUTE_SEGMENTS,
  BLUEPRINT_SUPPORT_LINES,
  getBlueprintArrivedFragmentIds,
  getBlueprintFocusIds,
  getBlueprintRouteStep,
  getOtpBlueprintCheckpoint,
} from './blueprintScene';

const otpSource = readFileSync(new URL('./OtpCodeInput.tsx', import.meta.url), 'utf8');
const statusBarSource = readFileSync(new URL('./AuthStatusBar.tsx', import.meta.url), 'utf8');
const intentSource = readFileSync(new URL('./IntentField.tsx', import.meta.url), 'utf8');
const authShellSource = readFileSync(new URL('./AuthShell.tsx', import.meta.url), 'utf8');
const blueprintSource = readFileSync(new URL('./blueprintScene.ts', import.meta.url), 'utf8');
const otpHookSource = readFileSync(new URL('../../hooks/auth/useEmailOtpLogin.ts', import.meta.url), 'utf8');
const authCss = readFileSync(new URL('./auth.css', import.meta.url), 'utf8');
const cloudLoginCss = readFileSync(new URL('../../pages/cloudLogin/CloudLoginPage.css', import.meta.url), 'utf8');
const cloudSource = readFileSync(new URL('../../pages/cloudLogin/index.tsx', import.meta.url), 'utf8');
const settingsSource = readFileSync(new URL('../../pages/settings/CloudLoginSettings.tsx', import.meta.url), 'utf8');
const zhCloudLocale = readFileSync(new URL('../../services/i18n/locales/zh-CN/cloudLogin.json', import.meta.url), 'utf8');
const enCloudLocale = readFileSync(new URL('../../services/i18n/locales/en-US/cloudLogin.json', import.meta.url), 'utf8');

describe('Flowy auth surface contract', () => {
  test('normalizes OTP input to numeric six digits', () => {
    expect(normalizeOtpCode('a1 2-3x456789')).toBe('123456');
    expect(normalizeOtpCode('12345')).toHaveLength(5);
    expect(normalizeOtpCode('123456')).toHaveLength(EMAIL_OTP_LENGTH);
    expect(EMAIL_OTP_LENGTH).toBe(6);
  });

  test('keeps the resend cooldown explicit and bounded', () => {
    expect(EMAIL_OTP_COOLDOWN_SECONDS).toBe(60);
    expect(authCss).toContain('font-variant-numeric: tabular-nums');
    expect(authCss).toContain('.flowy-otp__cell--group-start');
    expect(cloudSource).toContain('showVisual={false}');
    expect(settingsSource).toContain('showVisual={false}');
    expect(cloudSource).toContain('cooldownButton');
    expect(settingsSource).toContain('cooldownButton');
    expect(cloudSource).not.toContain('cooldownShort');
    expect(settingsSource).not.toContain('cooldownShort');
  });

  test('reserves the OTP status action slot to prevent layout shifts', () => {
    expect(statusBarSource).toContain('reserveActionSpace');
    expect(statusBarSource).toContain("data-empty={!hasAction ? 'true' : undefined}");
    expect(authCss).toContain('.flowy-auth-status--reserve-action');
    expect(authCss).toContain('visibility: hidden;');
    expect(cloudSource).toContain('reserveActionSpace');
    expect(settingsSource).toContain('reserveActionSpace');
    expect(cloudSource).toContain('cloud-login-input-slot');
    expect(cloudSource).toContain('cloud-login-status-slot');
    expect(cloudSource).toContain('cloud-login-action-slot');
    expect(settingsSource).toContain('flowy-settings-auth__input-slot');
    expect(settingsSource).toContain('flowy-settings-auth__status-slot');
    expect(settingsSource).toContain('flowy-settings-auth__action-slot');
  });

  test('changing the email starts a fresh cooldown window', () => {
    expect(otpHookSource).toContain('const changeEmail');
    expect(otpHookSource).toMatch(/const changeEmail[\s\S]{0,600}cooldown: 0/);
    expect(otpHookSource).toMatch(/const startLogin[\s\S]{0,600}cooldown: 0/);
  });

  test('uses one accessible input and six decorative visual cells', () => {
    expect(otpSource).toContain("type='text'");
    expect(otpSource).toContain("inputMode='numeric'");
    expect(otpSource).toContain("autoComplete='one-time-code'");
    expect(otpSource).toContain('maxLength={EMAIL_OTP_LENGTH}');
    expect(otpSource).toContain("aria-hidden='true'");
    expect(otpSource).toContain('onPaste');
    expect(otpSource).toContain('onSubmit');
    expect(otpSource).toContain('focusOnErrorReset');
    expect(otpSource).toContain('useLayoutEffect');
    expect(otpSource).toContain('preventScroll: true');
    expect(otpSource).toContain('setSelectionRange(0, 0)');
    expect(cloudSource).toContain("focusOnErrorReset={flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown'}");
    expect(settingsSource).toContain("focusOnErrorReset={flow.failureKind === 'invalid-code' || flow.failureKind === 'unknown'}");
  });

  test('keeps verification errors short and prevents orphan glyph wrapping', () => {
    expect(zhCloudLocale).toContain('"invalidCode": "验证码不正确，请重试"');
    expect(zhCloudLocale).toContain('"verificationUnavailable": "暂时无法确认，请稍后重试"');
    expect(enCloudLocale).toContain('"invalidCode": "That code didn’t work. Try again."');
    expect(enCloudLocale).toContain('"verificationUnavailable": "Unable to verify right now. Try again shortly."');
    expect(authCss).toContain('overflow-wrap: break-word;');
    expect(authCss).toContain('word-break: normal;');
    expect(authCss).toContain('text-wrap: pretty;');
    expect(authCss).not.toContain('overflow-wrap: anywhere;');
  });

  test('shows verification loading immediately while the OTP request is in flight', () => {
    expect(cloudSource).toContain("const isVerifying = flow.phase === 'verifying';");
    expect(cloudSource).toContain('isVerifying &&');
    expect(cloudSource).toContain('flowy-auth-status__spinner');
    expect(authCss).toContain('.flowy-auth-status__spinner');
  });

  test('keeps cloud legal links visibly wired but intentionally offline', () => {
    expect(zhCloudLocale).toContain('"privacy": "隐私政策"');
    expect(zhCloudLocale).toContain('"and": "和"');
    expect(zhCloudLocale).toContain('"terms": "服务条款"');
    expect(enCloudLocale).toContain('"privacy": "Privacy policy"');
    expect(enCloudLocale).toContain('"and": "and"');
    expect(enCloudLocale).toContain('"terms": "Terms of service"');
    expect(authShellSource).toContain('brandLegal?: ReactNode;');
    expect(authShellSource).toContain("flowy-auth-brand__legal");
    expect(cloudSource).toContain('const CloudLoginLegalLinks');
    expect(cloudSource).toContain("cloudLogin.legal.and");
    expect(cloudSource).toContain('brandLegal={<CloudLoginLegalLinks />}');
    expect(cloudSource).toContain("href='#'");
    expect(cloudSource).toContain("aria-disabled='true'");
    expect(cloudSource).toContain('event.preventDefault()');
    expect(cloudSource).toContain('flowy-auth-brand__legal-separator');
    expect(cloudSource).not.toContain('footer={<CloudLoginLegalLinks />}');
    expect(cloudSource).not.toContain('cloudLogin.footerPrimary');
    expect(cloudSource).not.toContain('cloudLogin.footerSecondary');
    expect(authCss).toContain('.flowy-auth-brand__legal');
    expect(authCss).toContain('.flowy-auth-brand__legal-link');
    expect(authCss).not.toContain('.flowy-auth-footer__link');
  });

  test('classifies verification failures without exposing backend text', () => {
    const backendError = (status: number, message: string, code = 'INTERNAL_ERROR') => ({
      name: 'BackendHttpError',
      status,
      code,
      backendMessage: message,
      body: { error: message, code },
    });

    expect(classifyOtpVerificationError(backendError(500, 'Internal error: API error 400: invalid code'))).toBe('invalid-code');
    expect(classifyOtpVerificationError(backendError(422, 'Verification code is invalid', 'CLOUD_OTP_INVALID_CODE'))).toBe('invalid-code');
    expect(classifyOtpVerificationError({
      status: 422,
      code: 'CLOUD_OTP_INVALID_CODE',
      body: { error: 'Verification code is invalid', code: 'CLOUD_OTP_INVALID_CODE' },
    })).toBe('invalid-code');
    expect(classifyOtpVerificationError(backendError(400, '验证码不正确', 'LEGACY_BAD_REQUEST'))).toBe('invalid-code');
    expect(classifyOtpVerificationError(backendError(502, 'upstream unavailable'))).toBe('transport');
    expect(classifyOtpVerificationError(new Error('Failed to fetch'))).toBe('transport');
    expect(classifyOtpVerificationError(backendError(500, 'unclassified backend failure'))).toBe('unknown');
    expect(classifyOtpVerificationError(backendError(410, 'otp session expired'))).toBe('session-expired');
    expect(classifyOtpVerificationError(backendError(429, 'too many attempts'))).toBe('session-expired');
    expect(isTerminalLoginFailureResponse({ status: 'failed', error: 'provider failure' })).toBe(true);
    expect(otpHookSource).toContain('if (isTerminalLoginFailureResponse(response))');
    expect(otpHookSource).toContain("failureKind: 'verification-failed'");
    expect(otpHookSource).toContain("message: t('cloudLogin.errors.verificationUnavailable')");
    expect(getOtpRecoveryAction('invalid-code')).toBe(null);
    expect(getOtpRecoveryAction('unknown')).toBe(null);
    expect(getOtpRecoveryAction('session-expired')).toBe('resend-code');
    expect(getOtpRecoveryAction('verification-failed')).toBe('resend-code');
    expect(getOtpRecoveryAction('transport')).toBe('retry-verification');
    expect(getOtpRecoveryAction('verification-pending')).toBe('retry-verification');
  });

  test('renders a deterministic SVG blueprint instead of a particle or 3D scene', () => {
    for (const marker of [
      'viewBox={BLUEPRINT_VIEWBOX}',
      'aria-hidden=\'true\'',
      'prefers-reduced-motion',
      'visibilitychange',
      'requestAnimationFrame',
      'cancelAnimationFrame',
      'onPointerEnter',
      'getBlueprintFocusIds',
      'getBlueprintArrivedFragmentIds',
      'BLUEPRINT_SUPPORT_LINES',
      'BLUEPRINT_ROUTE_SEGMENTS',
      'pathLength=\'1\'',
      'data-blueprint-step',
      'data-blueprint-phase',
    ]) {
      expect(intentSource).toContain(marker);
    }
    for (const marker of [
      '<canvas',
      'document.createElement(\'canvas\')',
      'GALAXY_',
      'Three',
      'WebGL',
      'Math.random()',
      'setInterval',
      'requestAnimationFrame(draw',
    ]) {
      expect(intentSource).not.toContain(marker);
    }
    expect(intentSource).not.toContain('pointerdown');
    expect(authCss).toContain('.flowy-blueprint');
    expect(authCss).not.toContain('flowy-blueprint__result-frame');
    expect(intentSource).not.toContain('BLUEPRINT_RESULT_FRAME');
    expect(intentSource).not.toContain('BLUEPRINT_MAIN_PATH');
    expect(intentSource).not.toContain('BLUEPRINT_SECONDARY_PATH');
    expect(intentSource).not.toContain('flowy-blueprint__secondary-path');
    expect(intentSource).toContain('flowy-blueprint__execution-cursor');
    expect(intentSource).toContain('flowy-blueprint__fragment-detail');
    expect(intentSource).not.toContain('flowy-blueprint__fragment-micro');
    expect(intentSource).not.toContain('flowy-blueprint__fragment-inset');
    expect(intentSource).not.toContain("kind: 'cell'");
    expect(intentSource).not.toContain("kind: 'status'");
    expect(intentSource).not.toContain('transitionKey');
    expect(intentSource).not.toContain('innerX + 18} ${innerY + 18}');
    expect(authCss).not.toContain('animation: flowy-blueprint-state');
    expect(authCss).toContain('@media (prefers-reduced-motion: reduce)');
  });

  test('keeps the blueprint composition hand-placed and bounded', () => {
    expect(BLUEPRINT_FRAGMENTS).toHaveLength(15);
    expect(BLUEPRINT_ROUTE_SEGMENTS).toHaveLength(7);
    expect(BLUEPRINT_SUPPORT_LINES).toHaveLength(3);
    expect(BLUEPRINT_AMBIENT_MARKS).toHaveLength(4);
    expect(BLUEPRINT_ROUTE_SEGMENTS.map((segment) => segment.step)).toEqual([1, 2, 3, 4, 5, 6, 7]);
    expect(new Set(BLUEPRINT_FRAGMENTS.map((fragment) => fragment.id)).size).toBe(BLUEPRINT_FRAGMENTS.length);
    expect(new Set(BLUEPRINT_ROUTE_SEGMENTS.map((segment) => segment.id)).size).toBe(BLUEPRINT_ROUTE_SEGMENTS.length);
    expect(BLUEPRINT_FRAGMENTS.filter((fragment) => fragment.role === 'primary')).toHaveLength(10);
    expect(BLUEPRINT_FRAGMENTS.filter((fragment) => fragment.role === 'companion')).toHaveLength(5);
    expect(BLUEPRINT_FRAGMENTS.every((fragment) => (
      fragment.x >= 0
      && fragment.y >= 0
      && fragment.x + fragment.width <= 1000
      && fragment.y + fragment.height <= 680
      && fragment.routeStep >= 0
      && fragment.routeStep <= 7
      && Number.isFinite(fragment.rotation)
    ))).toBe(true);
    expect(new Set(BLUEPRINT_FRAGMENTS.map((fragment) => fragment.variant))).toEqual(new Set([
      'command-lines',
      'file-sheet',
      'terminal-rows',
      'browser-pane',
      'diff-block',
      'table-grid',
      'summary-bars',
      'review-check',
    ]));
    const fragmentIds = new Set(BLUEPRINT_FRAGMENTS.map((fragment) => fragment.id));
    expect(BLUEPRINT_ROUTE_SEGMENTS.every((segment) => segment.path.startsWith('M '))).toBe(true);
    expect(BLUEPRINT_ROUTE_SEGMENTS.every((segment) => segment.fragmentIds.every((id) => fragmentIds.has(id)))).toBe(true);
    expect(BLUEPRINT_SUPPORT_LINES.every((line) => line.fragmentIds.length >= 1)).toBe(true);
    expect(BLUEPRINT_SUPPORT_LINES.every((line) => line.fragmentIds.every((id) => (
      BLUEPRINT_FRAGMENTS.some((fragment) => fragment.id === id)
    )))).toBe(true);
    expect(BLUEPRINT_SUPPORT_LINES.every((line) => line.path.startsWith('M '))).toBe(true);
    expect(BLUEPRINT_SUPPORT_LINES.map((line) => line.path)).toEqual([
      'M 322 200 L 348 200 L 356 166 L 408 173',
      'M 778 133 L 796 163',
      'M 373 445 H 438 V 501 H 487',
    ]);
    expect(BLUEPRINT_SUPPORT_LINES.every((line) => line.fragmentIds.length === 2)).toBe(true);
    expect(BLUEPRINT_SUPPORT_LINES[1].path).not.toContain(' V ');
    expect(BLUEPRINT_SUPPORT_LINES[2].path).toContain('H 438 V 501 H 487');
    expect(BLUEPRINT_ROUTE_SEGMENTS.every((segment, index) => (
      index === 0 || segment.step > BLUEPRINT_ROUTE_SEGMENTS[index - 1].step
    ))).toBe(true);
    const hasBoundingBoxOverlap = (left: (typeof BLUEPRINT_FRAGMENTS)[number], right: (typeof BLUEPRINT_FRAGMENTS)[number]) => (
      left.x < right.x + right.width
      && left.x + left.width > right.x
      && left.y < right.y + right.height
      && left.y + left.height > right.y
    );
    expect(BLUEPRINT_FRAGMENTS.some((fragment, index) => (
      BLUEPRINT_FRAGMENTS.slice(index + 1).some((other) => hasBoundingBoxOverlap(fragment, other))
    ))).toBe(false);
    expect(BLUEPRINT_FRAGMENTS.filter((fragment) => fragment.role === 'primary').every((fragment) => (
      fragment.details && fragment.details.length >= 1
    ))).toBe(true);
    expect(BLUEPRINT_FRAGMENTS.every((fragment) => (
      (fragment.details ?? []).every((detail) => ['line', 'dot', 'check'].includes(detail.kind))
    ))).toBe(true);
    const primaryCheckDetails = BLUEPRINT_FRAGMENTS.flatMap((fragment) => (
      fragment.role === 'primary'
        ? (fragment.details ?? []).filter((detail) => detail.kind === 'check').map((detail) => ({ fragment, detail }))
        : []
    ));
    expect(primaryCheckDetails.map(({ fragment }) => fragment.id)).toEqual([
      'file',
      'browser',
      'table',
      'review-summary',
    ]);
    expect(primaryCheckDetails.map(({ fragment, detail }) => `${fragment.id}:${detail.x}:${detail.y}`)).toEqual([
      'file:72:52',
      'browser:116:45',
      'table:72:26',
      'review-summary:96:55',
    ]);
    expect(primaryCheckDetails.every(({ detail }) => detail.emphasis === 'accent')).toBe(true);
    expect(BLUEPRINT_FRAGMENTS.every((fragment) => (
      (fragment.details ?? []).every((detail, index, details) => (
        index === 0 || detail.revealAt >= details[index - 1].revealAt
      ))
    ))).toBe(true);
    expect(blueprintSource).not.toContain('Math.random()');
    expect(blueprintSource).not.toContain('GALAXY');
    expect(blueprintSource).not.toContain('BLUEPRINT_RESULT_FRAME');
    expect(blueprintSource).not.toContain('BLUEPRINT_SECONDARY_PATH');
    expect(getBlueprintArrivedFragmentIds(0)).toEqual([]);
    expect(getBlueprintArrivedFragmentIds(2)).toEqual(['browser-result', 'command', 'file']);
    expect(getBlueprintArrivedFragmentIds(6)).toHaveLength(9);
    expect(getBlueprintArrivedFragmentIds(7)).toHaveLength(10);
    expect(getBlueprintArrivedFragmentIds(7)).not.toContain('companion-terminal');
    expect(getBlueprintFocusIds(268, 122)).not.toContain('companion-terminal');
  });

  test('maps cloud and local auth progress to the same visual language', () => {
    expect(getBlueprintRouteStep('cloud', 'idle', 0)).toBe(0);
    expect(getBlueprintRouteStep('cloud', 'input', 0)).toBe(0);
    expect(getBlueprintRouteStep('cloud', 'input', 0.5)).toBe(1);
    expect(getBlueprintRouteStep('cloud', 'code-sent', 0)).toBe(2);
    expect(getBlueprintRouteStep('cloud', 'verifying', 0)).toBe(7);
    expect(getBlueprintRouteStep('cloud', 'success', 0)).toBe(7);
    expect(getBlueprintRouteStep('cloud', 'input', 0, 4)).toBe(4);
    expect(getBlueprintRouteStep('local', 'idle', 0)).toBe(0);
    expect(getBlueprintRouteStep('local', 'input', 0.1)).toBe(1);
    expect(getBlueprintRouteStep('local', 'input', 0.5)).toBe(2);
    expect(getBlueprintRouteStep('local', 'input', 1)).toBe(3);
    expect([0, 1, 2, 3, 4, 5, 6].map(getOtpBlueprintCheckpoint)).toEqual([0, 1, 1, 2, 2, 3, 4]);
  });

  test('keeps theme, responsive, and semantic token contracts local to the auth surface', () => {
    for (const token of [
      '--auth-blueprint-canvas',
      '--auth-blueprint-ink',
      '--auth-blueprint-muted',
      '--auth-blueprint-hairline',
      '--auth-blueprint-surface',
      '--auth-blueprint-backplate',
      '--auth-blueprint-surface-quiet',
      '--auth-blueprint-line-quiet',
      '--auth-blueprint-line-document',
      '--auth-blueprint-accent',
      '--auth-blueprint-accent-soft',
      '--auth-blueprint-danger',
      '--auth-blueprint-motion-route',
      '--auth-blueprint-motion-document',
      '--auth-blueprint-motion-cursor',
      '--auth-blueprint-motion-complete',
    ]) {
      expect(authCss).toContain(token);
    }
    expect(authCss).toContain('color-mix(in srgb, var(--flowy-canvas) 90%, var(--flowy-text-primary) 10%)');
    expect(authCss).toContain('[data-theme=\'dark\'] .flowy-auth-brand');
    expect(authCss).toContain('@media (max-width: 899px)');
    expect(authCss).toContain('@media (max-width: 639px)');
    expect(authCss).toContain('.flowy-blueprint__support-lines');
    expect(authCss).not.toContain('.flowy-blueprint__companion-path');
    expect(intentSource).not.toContain('BLUEPRINT_COMPANION_PATHS');
    expect(intentSource).not.toContain('dividerX');
    expect(authCss).toContain('.flowy-blueprint__ambient-marks');
    expect(authCss).not.toContain('.flowy-blueprint__fragment:nth-child');
    expect(authCss).toContain('.flowy-blueprint__fragment.is-arrived');
    expect(authCss).toContain('.flowy-intent-field.is-page-hidden');
    expect(authCss).toContain('fill: var(--auth-blueprint-backplate)');
    expect(authCss).toContain('.flowy-blueprint__route-segment-active');
    expect(authCss).not.toContain('.flowy-blueprint__fragment-micro');
    expect(authCss).not.toContain('.flowy-blueprint__fragment-inset');
    expect(authCss).toContain('stroke-dashoffset var(--auth-blueprint-motion-route)');
    expect(authCss).toContain('fill-opacity var(--auth-blueprint-motion-document)');
    expect(authCss).toContain('stroke-opacity var(--auth-blueprint-motion-document)');
    expect(authCss).toContain('white-space: nowrap;');
    expect(intentSource).toContain("window.setTimeout(moveRoute, 400)");
    expect(intentSource).toContain('const direction = currentStep < targetStep ? 1 : -1');
    expect(intentSource).toContain('const motionDisabled = reducedMotion || compactViewport || !finePointer;');
    expect(intentSource).toContain("dur='480ms'");
    expect(authCss).toContain('.flowy-intent-field.is-reduced-motion .flowy-blueprint__fragment-shell');
    expect(cloudSource).toContain('activationLevel={activationLevel}');
    expect(cloudSource).toContain('blueprintStep={blueprintStepOverride}');
    expect(cloudSource).toContain('inputEnergy={emailInputEnergy}');
    expect(cloudSource).toContain('const emailInputEnergy');
    expect(cloudSource).toContain('handleEmailChange');
    expect(cloudSource).toContain('Math.min(EMAIL_OTP_LENGTH, flow.code.length)');
    expect(cloudSource).toContain('setBlueprintStep((2 + checkpoint) as BlueprintRouteStep)');
    expect(cloudSource).not.toContain('Math.max(previous, 2 + checkpoint)');
  });

  test('keeps cloud auth hooks behind a stable checking boundary and navigates once', () => {
    expect(cloudSource).toContain('const CloudLoginFlow');
    expect(cloudSource).toContain('const CloudLoginTransition');
    expect(cloudSource).toContain("status === 'checking'");
    expect(cloudSource).toContain('isCompleting');
    expect(cloudSource).toContain('isSessionReady');
    expect(cloudSource).toContain('cloud-login-page--transition');
    expect(cloudSource).toContain('preloadGuidPathChunk');
    expect(cloudSource).toContain('.catch(() => undefined)');
    expect(cloudSource).toContain('cloudLogin.legal.privacy');
    expect(cloudSource).toContain('cloudLogin.legal.terms');
    expect(cloudSource).toContain('preventPlaceholderNavigation');
    expect(cloudSource).toContain("aria-busy={onRetry ? 'false' : 'true'}");
    expect(cloudSource).toContain('workspacePreparationFailed');
    expect(cloudSource).toContain('retryWorkspacePreparation');
    expect(cloudSource).toContain("refreshResult !== 'authenticated'");
    expect(cloudLoginCss).toContain('.cloud-login-transition__spinner');
    expect(cloudSource).toContain('<CloudLoginFlow');
    expect(cloudSource).toContain('justLoggedInRef.current = false');
    expect(cloudSource).toContain('navigate(\'/guid\', { replace: true })');
    expect(cloudSource).not.toContain('cloudLogin.footerPrimary');
    expect(cloudSource).not.toContain('cloudLogin.footerSecondary');
    expect(cloudSource).not.toContain('cloud-login-page--checking');
    expect(cloudSource).not.toContain('if (status === \'checking\') return null;');
  });

  test('shares the OTP controller across cloud entry points', () => {
    expect(cloudSource).toContain('useEmailOtpLogin');
    expect(settingsSource).toContain('useEmailOtpLogin');
    expect(cloudSource).not.toContain('maxLength={8}');
    expect(settingsSource).not.toContain('maxLength={8}');
    expect(cloudSource).not.toContain('String(e)');
    expect(cloudSource).toContain("flow.recoveryAction === 'retry-verification'");
    expect(settingsSource).toContain("flow.recoveryAction === 'retry-verification'");
    expect(cloudSource).not.toContain("flow.phase === 'transport-error' && flow.code.length === 6");
    expect(settingsSource).not.toContain("flow.phase === 'transport-error' && flow.code.length === 6");
    expect(cloudSource).not.toContain('cloudLogin.brandEyebrow');
    expect(cloudSource).not.toContain('cloudLogin.login.resendIn');
    expect(settingsSource).not.toContain('cloudLogin.login.resendIn');
  });
});
