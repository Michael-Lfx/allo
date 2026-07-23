

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ComposerSubmitCluster', () => {
  test('empty draft renders speech without always-on disabled send', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('hasDraft')).toBe(true);
    expect(source.includes('SpeechInputButton')).toBe(true);
    expect(source.includes("data-testid={sendTestId}")).toBe(true);
    expect(source.includes('showSendButton = hasDraft && !autoWorkMode')).toBe(true);
  });

  test('autoWorkMode renders robot button alongside speech', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('autoWorkMode')).toBe(true);
    expect(source.includes('Robot')).toBe(true);
    expect(source.includes('composer-autowork-btn')).toBe(true);
  });

  test('keeps the rightmost circle slot stable when draft appears', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('showSecondarySpeech')).toBe(true);
    expect(source.includes('showPrimaryFilledSpeech')).toBe(true);
    expect(source.includes("variant='inline'")).toBe(true);
    expect(source.includes("variant='filled'")).toBe(true);
    expect(source.includes('composer-submit-cluster__speech-secondary')).toBe(true);
  });
});
