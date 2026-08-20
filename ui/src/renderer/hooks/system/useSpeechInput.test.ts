/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const readSource = (url: URL): string => readFileSync(url, 'utf8');

describe('speech input recording handling', () => {
  test('keeps the microphone button dedicated to live recording', () => {
    const button = readSource(
      new URL('../../components/chat/SpeechInputButton.tsx', import.meta.url)
    );

    expect(button.includes("type='file'")).toBe(false);
    expect(button.includes('fileInputRef')).toBe(false);
    expect(button.includes('transcribeFile')).toBe(false);
  });

  test('declares the macOS microphone privacy reason required by the desktop WebView', () => {
    const plist = readSource(new URL('../../../../../apps/desktop/Info.plist', import.meta.url));

    expect(plist.includes('<key>NSMicrophoneUsageDescription</key>')).toBe(true);
    expect(plist.includes('voice input')).toBe(true);
  });

  test('submits the browser recording to the configured cloud service', () => {
    const hook = readSource(new URL('./useSpeechInput.ts', import.meta.url));

    expect(hook.includes('await transcribeBlob(blob)')).toBe(true);
    expect(hook.includes('convertRecordedAudioToWav')).toBe(false);
  });

  test('does not hide the microphone behind speech-to-text settings', () => {
    const button = readSource(
      new URL('../../components/chat/SpeechInputButton.tsx', import.meta.url)
    );

    expect(button.includes("from '@/renderer/services/speechToTextConfig';")).toBe(false);
    expect(button.includes('getSpeechToTextConfig')).toBe(false);
    expect(button.includes('isSpeechInputEnabled')).toBe(false);
    expect(button.includes('useProvidersQuery')).toBe(false);
    expect(button.includes('if (hidden)')).toBe(true);
  });

  test('does not opt the desktop multipart request into credentialed CORS', () => {
    const service = readSource(
      new URL('../../services/SpeechToTextService.ts', import.meta.url)
    );
    const bridge = readSource(new URL('../../../common/adapter/httpBridge.ts', import.meta.url));

    expect(service.includes('xhr.withCredentials = true')).toBe(false);
    expect(service.includes("credentials: 'include'")).toBe(false);
    expect(service.includes('speechToText.transcribe.invoke')).toBe(true);

    const multipartStart = bridge.indexOf('export async function httpMultipartRequest');
    const multipart = bridge.slice(multipartStart, bridge.indexOf('export function withResponseMap'));
    expect(multipartStart).toBeGreaterThan(-1);
    expect(multipart.includes("buildBackendAuthHeaders('POST')")).toBe(true);
    expect(multipart.includes('withCredentials')).toBe(false);
    expect(multipart.includes("credentials: 'include'")).toBe(false);
  });
});
