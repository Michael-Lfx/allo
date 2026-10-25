/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import VoiceCallDock from './VoiceCallDock';
import VoiceCallDockDetails from './VoiceCallDockDetails';
import { resolveVoiceDockPresentation } from './voiceCallDockState';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

const dictionaries = { 'zh-CN': zh, 'en-US': en } as const;

async function localized(child: React.ReactNode, locale: 'zh-CN' | 'en-US') {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: { [locale]: { translation: { voice: dictionaries[locale] } } },
    initImmediate: false,
  });
  return renderToStaticMarkup(<I18nextProvider i18n={i18n}>{child}</I18nextProvider>);
}

const snapshot = (overrides: Partial<VoiceSnapshot> = {}): VoiceSnapshot =>
  ({
    instance_id: 'host',
    seq: 1,
    session_id: 'call',
    phase: 'listening',
    connection: 'connected',
    enabled: true,
    acoustic_ready: true,
    dialogue_ready: true,
    playback_ready: true,
    binding: {
      session_id: 'call',
      binding_id: 'binding',
      conversation_id: 'p',
      workspace: null,
    },
    audio: {
      rms_milli: 360,
      peak_milli: 800,
      frames_received: 20,
      last_frame_at_ms: Date.now(),
      capture_state: 'receiving',
      speech_detected: true,
      asr_ready: true,
      kws_ready: true,
      vad_ready: true,
      barge_in_ready: false,
      barge_in_reason: null,
    },
    projected_turn_ids: [],
    conversation: [{ turn_id: 'private-turn', model_text: 'private router JSON' }],
    error: null,
    ended_reason: null,
    user_text: 'private user text',
    assistant_text: 'private assistant text',
    task: null,
    queue: [],
    confirmation: null,
    ...overrides,
  }) as VoiceSnapshot;

const task = (conversationId: string, commandId: string, status: 'accepted' | 'queued' | 'unknown') => ({
  command_id: commandId,
  session_id: 'call',
  acoustic_turn_id: `turn-${commandId}`,
  conversation_id: conversationId,
  message_id: `msg-${commandId}`,
  runtime_turn_id: null,
  status,
  result_ok: null,
  result_text: null,
  error_code: null,
});

for (const locale of ['zh-CN', 'en-US'] as const) {
  const dictionary = dictionaries[locale];
  const detailViews = resolveVoiceDockPresentation({
    conversationId: 'p',
    snapshot: snapshot({
      phase: 'idle',
      ended_reason: 'user_stop',
      task: { ...task('q', 'q-task', 'accepted'), result_text: 'Q private result' },
      queue: [
        {
          task: task('p', 'p-queue', 'queued'),
          display_text: 'P queued B',
          workspace: 'D:/p',
          position: 1,
        },
      ],
    }),
    historyFailed: true,
  });

  test(`${locale}: P queue stays visible while Q runs, after hangup, in both themes`, async () => {
    const scene = snapshot({
      phase: 'idle',
      ended_reason: 'user_stop',
      task: { ...task('q', 'q-task', 'accepted'), result_text: 'Q private result' },
      queue: [
        {
          task: task('p', 'p-queue', 'queued'),
          display_text: 'P queued B',
          workspace: 'D:/p',
          position: 1,
        },
      ],
    });
    for (const theme of ['light', 'dark']) {
      const html = await localized(
        <div data-theme={theme}>
          <VoiceCallDock conversationId='p' snapshot={scene} historyFailed />
        </div>,
        locale
      );
      expect(html).toContain('data-testid="voice-call-dock"');
      expect(html).toContain(dictionary.dock.notInCall);
      expect(html).toContain(dictionary.dock.syncFailed);
      expect(html).toContain('aria-expanded="false"');
      expect(html).toContain('data-testid="voice-call-dock-details-toggle"');
      expect(html).toContain(dictionary.dock.detailsWithCount.replace('{{count}}', '2'));
      expect(html).not.toContain('Q private result');
      expect(html).not.toContain('P queued B');
      expect(html).not.toContain('<details');
    }

    const details = await localized(
      <VoiceCallDockDetails
        conversationId='p'
        snapshot={scene}
        details={detailViews.details}
        onRetryHistory={async () => undefined}
      />,
      locale
    );
    expect(details).toContain('P queued B');
    expect(details).toContain(dictionary.task.cancel);
    expect(details).toContain(dictionary.composer.historyFailed);
    expect(details).toContain(dictionary.composer.retryHistory);
    expect(details).not.toContain('Q private result');
    expect(details).not.toContain('voice.');
  });

  test(`${locale}: every acoustic phase renders one concise translated state`, async () => {
    const expected: Record<string, string> = {
      idle: dictionary.dock.waitingWake,
      listening: dictionary.dock.listening,
      transcribing: dictionary.dock.transcribing,
      deciding: dictionary.dock.deciding,
      synthesizing: dictionary.dock.synthesizing,
      speaking: dictionary.dock.speaking,
      follow_up: dictionary.dock.followUp,
      text_follow_up: dictionary.dock.textFollowUp,
      awaiting_confirmation: dictionary.dock.awaitingApproval,
      listening_confirm: dictionary.dock.listeningConfirm,
      dispatching: dictionary.dock.dispatching,
      error: dictionary.dock.unavailable,
    };
    for (const [phase, label] of Object.entries(expected)) {
      const html = await localized(
        <VoiceCallDock
          conversationId='p'
          snapshot={snapshot({ phase: phase as VoiceSnapshot['phase'] })}
        />,
        locale
      );
      expect(html).toContain(label);
      expect(html).not.toContain('D:/private-project');
      expect(html).not.toContain('private router JSON');
      expect(html).not.toContain('<details');
    }
  });

  test(`${locale}: status text is not a button and details are a separate control`, async () => {
    const html = await localized(
      <VoiceCallDock
        conversationId='p'
        snapshot={snapshot({
          task: task('p', 'p-task', 'unknown'),
        })}
      />,
      locale
    );
    expect(html).toMatch(/<span[^>]*data-testid="voice-call-dock-status"/);
    expect(html).toContain('data-testid="voice-call-dock-details-toggle"');
    expect(html).toMatch(/data-testid="voice-call-dock-details-toggle"[^>]*aria-expanded="false"/);
  });

  test(`${locale}: another conversation does not expose the active call or its content`, async () => {
    const html = await localized(
      <VoiceCallDock
        conversationId='b'
        snapshot={snapshot({
          task: { ...task('p', 'p-task', 'accepted'), result_text: 'P private result' },
          confirmation: {
            approval_id: 'approval-p',
            msg_id: 'msg-p',
            call_id: 'call-p',
            tool_name: 'read',
            arguments: { path: 'D:/private' },
            expires_at_ms: Date.now() + 60_000,
          },
        })}
        isActiveElsewhere
        onReturnToCall={() => undefined}
      />,
      locale
    );
    expect(html).toContain(dictionary.dock.activeElsewhere);
    expect(html).toContain(dictionary.entry.returnVoiceCall);
    expect(html).not.toContain('P private result');
    expect(html).not.toContain(dictionary.task.confirmationTitle);
    expect(html).not.toContain('D:/private');
  });

  test(`${locale}: barge-in is not advertised without both capability and capture`, async () => {
    const plain = await localized(
      <VoiceCallDock conversationId='p' snapshot={snapshot({ phase: 'speaking' })} />,
      locale
    );
    expect(plain).toContain(dictionary.dock.speaking);
    expect(plain).not.toContain(dictionary.dock.speakingInterruptible);

    const ready = await localized(
      <VoiceCallDock
        conversationId='p'
        snapshot={snapshot({
          phase: 'speaking',
          audio: { ...snapshot().audio!, barge_in_ready: true },
        })}
      />,
      locale
    );
    expect(ready).toContain(dictionary.dock.speakingInterruptible);
  });
}

test('refresh does not replace real status with an idle assertion', async () => {
  const html = await localized(<VoiceCallDock conversationId='p' snapshot={null} />, 'en-US');
  expect(html).toContain(en.dock.syncing);
  expect(html).not.toContain(en.dock.notInCall);
});



test('history failure with no snapshot keeps an aria-controls target without inventing a call', async () => {
  const html = await localized(
    <VoiceCallDock
      conversationId='p'
      snapshot={null}
      historyFailed
      onRetryHistory={async () => undefined}
    />,
    'en-US'
  );
  expect(html).toContain(en.dock.syncing);
  expect(html).not.toContain(en.dock.listening);
  expect(html).toMatch(/aria-controls="[^"]+"/);
  expect(html).toContain('data-testid="voice-call-dock-details"');
  expect(html).toContain('hidden=""');
});

test('dock is mounted outside the SendBox while the reusable top-right row remains', () => {
  const nomi = readFileSync(
    new URL('../conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url),
    'utf8'
  );
  const sendBox = readFileSync(
    new URL('../../components/chat/SendBox/index.tsx', import.meta.url),
    'utf8'
  );
  const css = readFileSync(new URL('./voiceCallDock.css', import.meta.url), 'utf8');

  const dockIndex = nomi.indexOf('<VoiceCallDock');
  const sendBoxIndex = nomi.indexOf('<SendBox');
  expect(dockIndex).toBeGreaterThan(-1);
  expect(sendBoxIndex).toBeGreaterThan(dockIndex);
  expect(nomi.includes('statusSlot=')).toBe(false);
  expect(sendBox.includes('statusSlot')).toBe(false);
  expect(sendBox.includes('topRightTools?: React.ReactNode')).toBe(true);
  expect(sendBox.includes("data-testid='sendbox-internal-status-row'")).toBe(true);
  expect(sendBox.includes("data-testid='sendbox-status-slot'")).toBe(false);
  expect(css.includes('margin-bottom: 12px')).toBe(true);
  expect(css.includes('position: absolute')).toBe(true);
});
