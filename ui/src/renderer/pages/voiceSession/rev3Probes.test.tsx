/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import ConversationRow from '../conversation/SessionList/ConversationRow';
import type { ConversationRowProps } from '../conversation/SessionList/types';
import { LayoutContext } from '@renderer/hooks/context/LayoutContext';
import VoiceComposerStatus from './VoiceComposerStatus';
import * as voice from '@common/adapter/voiceCompanion';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

async function localized(child: React.ReactNode, locale: 'zh-CN' | 'en-US') {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: locale === 'zh-CN' ? zh : en } } }, initImmediate: false });
  return renderToStaticMarkup(<I18nextProvider i18n={i18n}>{child}</I18nextProvider>);
}

for (const locale of ['zh-CN', 'en-US'] as const) {
  const dictionary = locale === 'zh-CN' ? zh : en;
  test(`${locale}: missing snapshot is syncing, not an assertion of idle`, async () => {
    const html = await localized(<VoiceComposerStatus conversationId='p' snapshot={null} />, locale);
    expect(html).toContain(dictionary.composer.pill.syncing);
    expect(html).not.toContain(dictionary.session.phase.idle);
  });
  test(`${locale}: stale listening phases cannot claim a working microphone`, async () => {
    const snapshot = { instance_id: 'host', seq: 1, session_id: 'call', phase: 'listening', connection: 'connected', acoustic_ready: true,
      binding: { session_id: 'call', binding_id: 'binding', conversation_id: 'p', workspace: null } } as voice.VoiceSnapshot;
    const offline = await localized(<VoiceComposerStatus conversationId='p' snapshot={{...snapshot, connection: 'offline'}} />, locale);
    expect(offline).toContain(dictionary.session.unavailable);
    expect(offline).not.toContain(dictionary.session.phase.listening);
    const ended = await localized(<VoiceComposerStatus conversationId='p' snapshot={{...snapshot, ended_reason: 'user_stop'}} />, locale);
    expect(ended).toContain(dictionary.session.phase.idle);
    const text = await localized(<VoiceComposerStatus conversationId='p' snapshot={{...snapshot, acoustic_ready: false}} />, locale);
    expect(text).toContain(dictionary.session.phase.text_follow_up);
  });
  test(`${locale}: voice wave survives rename, active state is accessible, and actions stay focusable`, async () => {
    const props = { conversation: { id: 'p', type: 'nomi', name: 'renamed', extra: { voice: true }, created_at: Date.now() },
      isCallActive: true, isGenerating: false, hasCompletionUnread: false, collapsed: false, tooltipEnabled: false,
      batchMode: false, checked: false, selected: false, menuVisible: false, onToggleChecked: () => {}, onConversationClick: () => {},
      onOpenMenu: () => {}, onMenuVisibleChange: () => {}, onEditStart: () => {}, onDelete: () => {}, onTogglePin: () => {}, getJobStatus: () => 'none',
    } as unknown as ConversationRowProps;
    const render = (active: boolean) => localized(<LayoutContext.Provider value={{ isMobile: false, siderCollapsed: false, setSiderCollapsed: () => {} }}>
      <ConversationRow {...props} isCallActive={active} />
    </LayoutContext.Provider>, locale);
    const active = await render(true);
    expect(active).toContain(`aria-label="${dictionary.entry.callActiveMarker}"`);
    expect(active).toContain('aria-haspopup="menu"');
    expect(active).toContain('group-focus-within:opacity-100');
    expect(active).not.toContain('hidden group-hover:flex');
    const idle = await render(false);
    expect(idle).toContain('lucide-audio-lines');
    expect(idle).not.toContain('i-icon-voice');
  });
}

test('a delayed seed from another subscriber cannot resurrect the globally ended call', async () => {
  const original = globalThis.fetch;
  let seedCount = 0;
  let releaseSlow!: (value: Response) => void;
  const slow = new Promise<Response>(resolve => { releaseSlow = resolve; });
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    if (String(input).endsWith('/session')) {
      if (++seedCount === 1) return slow;
      return Response.json({ instance_id: 'rev3-concurrent-host', seq: 20, session_id: null, phase: 'idle', ended_reason: 'user_stop', binding: null });
    }
    return new Response(new ReadableStream({ start() {} }), { headers: { 'Content-Type': 'text/event-stream' } });
  }) as typeof fetch;
  let offSlow: (() => void) | undefined;
  let offFast: (() => void) | undefined;
  try {
    const slowSnapshots: voice.VoiceSnapshot[] = [];
    const first = voice.subscribeVoiceSession(snapshot => { slowSnapshots.push(snapshot); }, false);
    offFast = await voice.subscribeVoiceSession(() => {}, false);
    expect(voice.getActiveVoiceCallConversationId()).toBeNull();
    releaseSlow(Response.json({ instance_id: 'rev3-concurrent-host', seq: 19, session_id: 'old-call', phase: 'listening', ended_reason: null,
      binding: { session_id: 'old-call', binding_id: 'old-binding', conversation_id: 'p', workspace: null } }));
    offSlow = await first;
    expect(voice.getActiveVoiceCallConversationId()).toBeNull();
    expect(slowSnapshots.at(-1)?.seq).toBe(20);
  } finally { offSlow?.(); offFast?.(); globalThis.fetch = original; }
});
