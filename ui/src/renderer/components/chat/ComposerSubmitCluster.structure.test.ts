

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ComposerSubmitCluster', () => {
  test('keeps the disabled send action visible for an empty draft', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('hasDraft')).toBe(true);
    expect(source.includes('SpeechInputButton')).toBe(true);
    expect(source.includes("data-testid={sendTestId}")).toBe(true);
    expect(source.includes('showSendButton = !autoWorkMode')).toBe(true);
    expect(source.includes('disabled={sendDisabled}')).toBe(true);
  });

  test('uses stop as the sole primary action while a conversation is running', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('const showStopButton = showStop && Boolean(onStop)')).toBe(true);
    expect(source.includes('const showSendButton = !autoWorkMode && !showStopButton')).toBe(true);
    expect(source.includes('const showSteerButton = showSteer && !showStopButton')).toBe(true);
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
