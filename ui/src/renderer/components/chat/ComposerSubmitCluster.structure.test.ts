

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { isValidElement, type MouseEvent as ReactMouseEvent, type ReactElement } from 'react';
import { ComposerStopButton } from './ComposerSubmitCluster';

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
    expect(source.includes("data-layout-part='send'")).toBe(true);
  });

  test('forwards React click detail 0, 1, and 2 through the rendered stop button', () => {
    const received: number[] = [];
    const button = ComposerStopButton({
      label: 'Stop',
      title: 'Stop generating',
      onStop: (clickDetail) => received.push(clickDetail),
    });

    expect(isValidElement(button)).toBe(true);
    const onClick = (button as ReactElement<{ onClick: (event: ReactMouseEvent<HTMLButtonElement>) => void }>).props
      .onClick;

    for (const detail of [0, 1, 2]) {
      onClick({ detail } as ReactMouseEvent<HTMLButtonElement>);
    }

    expect(received).toEqual([0, 1, 2]);
  });

  test('autoWorkMode renders robot button alongside speech', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('autoWorkMode')).toBe(true);
    expect(source.includes('Robot')).toBe(true);
    expect(source.includes('composer-autowork-btn')).toBe(true);
  });

  test('shows the microphone from claw availability instead of local STT settings', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('useClawAsrAvailable')).toBe(true);
    expect(source.includes('speechHidden')).toBe(true);
    expect(source.includes('getSpeechToTextConfig')).toBe(false);
    expect(source.includes('isSpeechInputEnabled')).toBe(false);
  });

  test('keeps the rightmost circle slot stable when draft appears', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('showSecondarySpeech')).toBe(true);
    expect(source.includes('showPrimaryFilledSpeech')).toBe(true);
    expect(source.includes("variant='inline'")).toBe(true);
    expect(source.includes("variant='filled'")).toBe(true);
    expect(source.includes('composer-submit-cluster__speech-secondary')).toBe(true);
  });

  test('passes the running state to the secondary microphone before rendering the cluster', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));

    expect(source.includes('const speechDisabled = disabled || loading || isUploading || (showStop && !hasDraft)')).toBe(true);
    expect(source.includes('const speechButtonProps = {')).toBe(true);
    expect(source.includes('disabled: speechDisabled')).toBe(true);
    expect(source.includes("<SpeechInputButton {...speechButtonProps} variant='inline' />")).toBe(true);
  });
});
