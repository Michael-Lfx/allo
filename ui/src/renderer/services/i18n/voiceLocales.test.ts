/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import enVoice from './locales/en-US/voice.json';
import zhVoice from './locales/zh-CN/voice.json';

type LocaleJson = Record<string, unknown>;

const flattenKeys = (locale: LocaleJson, prefix = ''): string[] =>
  Object.entries(locale).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object') return flattenKeys(value as LocaleJson, path);
    return [path];
  });

const getLocaleValue = (locale: LocaleJson, key: string): unknown => {
  let cursor: unknown = locale;
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor !== 'object' || !Object.prototype.hasOwnProperty.call(cursor, segment)) {
      return undefined;
    }
    cursor = (cursor as LocaleJson)[segment];
  }
  return cursor;
};

describe('voice companion locale bundle (C6 / T0-06)', () => {
  test('en-US and zh-CN expose the exact same key set', () => {
    expect(flattenKeys(zhVoice).sort()).toEqual(flattenKeys(enVoice).sort());
  });

  test('covers all 7 ModelInstallState statuses defined in baseline model-management', () => {
    const requiredStatuses = [
      'status.notInstalled',
      'status.probingSource',
      'status.downloading',
      'status.verifyingSha256',
      'status.ready',
      'status.failed',
      'status.removing',
    ];
    for (const key of requiredStatuses) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('covers all phase descriptions required by ui-motion specification', () => {
    const requiredPhases = [
      'phases.probingSource',
      'phases.downloading',
      'phases.downloadingFile',
      'phases.verifyingSha256',
      'phases.ready',
      'phases.removing',
      'phases.failed',
    ];
    for (const key of requiredPhases) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('covers download speed and progress indicators', () => {
    const progressKeys = [
      'progress.speed',
      'progress.percent',
      'progress.ratio',
      'progress.bytesRatio',
      'progress.approxSize',
    ];
    for (const key of progressKeys) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('covers all required buttons and actions including two-step deletion confirmation', () => {
    const actionKeys = [
      'actions.downloadAndEnable',
      'actions.retry',
      'actions.releaseSpace',
      'actions.confirmRelease',
      'actions.cancel',
      'actions.enableVoice',
      'actions.disableVoice',
    ];
    for (const key of actionKeys) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('covers error messages and failure alerts', () => {
    const errorKeys = [
      'errors.downloadFailed',
      'errors.verificationFailed',
      'errors.diskSpaceError',
      'errors.networkTimeout',
      'errors.pairingCodeInvalid',
      'errors.pairingCodeLocked',
      'errors.componentMissing',
    ];
    for (const key of errorKeys) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('covers Scene B pairing page copy', () => {
    const pairingKeys = [
      'pairing.title',
      'pairing.subtitle',
      'pairing.deviceStatus',
      'pairing.codePlaceholder',
      'pairing.verifying',
      'pairing.readyNotice',
      'pairing.successTitle',
      'pairing.successDesc',
    ];
    for (const key of pairingKeys) {
      expect(typeof getLocaleValue(enVoice, key)).toBe('string');
      expect(typeof getLocaleValue(zhVoice, key)).toBe('string');
    }
  });

  test('preserves interpolation placeholders symmetrically in both locales', () => {
    const interpolated: readonly [string, string][] = [
      ['phases.downloading', '{{current}}'],
      ['phases.downloading', '{{total}}'],
      ['phases.downloadingFile', '{{file}}'],
      ['phases.downloadingFile', '{{current}}'],
      ['phases.downloadingFile', '{{total}}'],
      ['phases.verifyingSha256', '{{current}}'],
      ['phases.verifyingSha256', '{{total}}'],
      ['progress.speed', '{{speed}}'],
      ['progress.percent', '{{percent}}'],
      ['progress.ratio', '{{current}}'],
      ['progress.ratio', '{{total}}'],
      ['progress.bytesRatio', '{{downloaded}}'],
      ['progress.bytesRatio', '{{total}}'],
      ['errors.pairingCodeLocked', '{{remaining}}'],
    ];

    for (const [key, placeholder] of interpolated) {
      const enVal = String(getLocaleValue(enVoice, key));
      const zhVal = String(getLocaleValue(zhVoice, key));
      expect(enVal).toContain(placeholder);
      expect(zhVal).toContain(placeholder);
    }
  });
});
