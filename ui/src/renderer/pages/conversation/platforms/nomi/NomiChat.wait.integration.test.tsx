import * as bunTest from 'bun:test';
const { expect } = bunTest;
const mock = (bunTest as unknown as { mock: any }).mock;
const afterAll = (bunTest as unknown as { afterAll: (cb: () => void | Promise<void>) => void }).afterAll;
// @ts-expect-error jsdom has no local declaration in this workspace
import { JSDOM } from 'jsdom';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';
import { PreviewProvider } from '@renderer/pages/conversation/Preview/context';
import { CloudAuthContext } from '@renderer/hooks/context/CloudAuthContext';
import { CreditsContext } from '@renderer/hooks/context/CreditsContext';
import { ThemeContext } from '@renderer/hooks/context/ThemeContext';
import zhVoice from '@renderer/services/i18n/locales/zh-CN/voice.json';

const turnActivity = {
  running: false, hasHydratedRunningState: true, tokenUsage: null,
  presentation: { phase: 'idle', streamFinished: true },
  setActiveMsgId: () => {}, markTurnAccepted: () => {}, notifyLocalSubmit: () => {},
  notifyAccepted: () => {}, notifyFailed: () => {}, reconcilePublicDeliveryReplay: () => {},
  reconcileAfterStreamTerminal: () => {}, setWaitingResponse: () => {}, resetState: () => {},
  confirmStopped: () => {}, restoreRunningAfterStopFailure: () => {},
  getTurnStartGeneration: () => 0, getTurnCompletionGeneration: () => 0,
  activeTurnId: undefined, activeRequestMessageId: undefined, stopNotice: null,
};
mock.module('./useNomiMessage', () => ({ useNomiMessage: () => turnActivity }));
mock.module('@renderer/pages/voiceSession/useVoiceSnapshot', () => ({
  useVoiceSnapshot: () => ({ snapshot: null, failed: false }),
  usePendingVoiceCallConversationId: () => null,
}));

const CONVERSATION_ID = '019b0000-0000-7000-8000-000000000001';
let taskStatus: 'accepted' | 'completed' = 'accepted';
let historyItems: unknown[] = [];
let resolveSend: ((value: unknown) => void) | null = null;
let resolveTaskGate: (() => void) | null = null;
let taskGateUsed = false;
let sendStarted = false;
const sendTask = {
  command_id: 'cmd-page-wait',
  session_id: 'session-page',
  acoustic_turn_id: 'turn-page',
  conversation_id: CONVERSATION_ID,
  message_id: '019b0000-0000-7000-8000-000000000912',
  runtime_turn_id: 'runtime-page',
  status: 'accepted',
  result_ok: null,
  result_text: null,
  error_code: null,
};

const response = (body: unknown) =>
  new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } });
Object.defineProperty(globalThis, 'fetch', {
  configurable: true,
  value: async (input: unknown) => {
    const url = typeof input === 'string' ? input : (input as any)?.url ?? String(input);
    if (url.includes('/api/voice/companion/messages')) {
      sendStarted = true;
      return new Promise((resolve) => {
        resolveSend = (body) => resolve(response(body));
      });
    }
    if (url.includes('/api/voice/companion/tasks/')) {
      if (!taskGateUsed) {
        taskGateUsed = true;
        return new Promise((resolve) => {
          resolveTaskGate = () => resolve(response({ ...sendTask, status: taskStatus }));
        });
      }
      return response({ ...sendTask, status: taskStatus });
    }
    if (url.includes('/api/conversations/') && url.includes('/messages')) {
      return response({ items: historyItems, has_more: false, next_cursor: null });
    }
    if (url.includes('/api/conversations/') && url.includes('/artifacts')) return response([]);
    if (url.includes('/api/mcp/servers')) return response([]);
    if (url.includes('pending')) return response([]);
    return response({});
  },
});

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/' });
const w = dom.window;
for (const name of ['Element','HTMLElement','Node','Event','CustomEvent','MouseEvent','KeyboardEvent','DocumentFragment','Blob','File','localStorage','sessionStorage','location','history'] as const) {
  if ((w as any)[name]) Object.defineProperty(globalThis, name, { configurable: true, value: (w as any)[name] });
}
for (const [name, value] of [
  ['window', w], ['document', w.document], ['navigator', w.navigator],
  ['getComputedStyle', w.getComputedStyle.bind(w)],
  ['requestAnimationFrame', (cb: any) => setTimeout(() => cb(Date.now()), 16)],
  ['cancelAnimationFrame', (id: any) => clearTimeout(id)],
  ['ResizeObserver', class { observe() {} unobserve() {} disconnect() {} }],
  ['IntersectionObserver', class { observe() {} unobserve() {} disconnect() {} }],
  ['matchMedia', () => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })],
  ['IS_REACT_ACT_ENVIRONMENT', true],
] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
w.requestAnimationFrame = (cb: any) => setTimeout(() => cb(Date.now()), 16) as any;
w.cancelAnimationFrame = (id: any) => clearTimeout(id);
w.HTMLElement.prototype.scrollTo = () => {};
w.Element.prototype.scrollTo = () => {};
afterAll(() => dom.window.close());

(bunTest.test as unknown as (name: string, fn: () => Promise<void>, timeout?: number) => void)('actual NomiChat composer shows the left wait through 56s and hands off to one durable reply', async () => {
  const i18n = createInstance();
  await i18n.init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: { voice: zhVoice } } }, initImmediate: false });
  const NomiChat = (await import('./NomiChat')).default;
  const modelSelection = {
    current_model: { id: 'p', name: 'P', use_model: 'm' },
    isCurrentModelAvailable: true,
    isModelCatalogLoading: false,
    refreshModelCatalog: () => {},
    providers: [],
    getAvailableModels: () => [],
    handleSelectModel: async () => {},
    formatModelLabel: (_p: any, m?: string) => m ?? '',
    getDisplayModelName: (m?: string) => m ?? '',
    modelPicker: { autoModels: [], cloudModels: [], otherProviderGroups: [] },
  };
  const container = w.document.getElementById('root')!;
  const root = createRoot(container);
  try {
    await act(async () => {
      root.render(
        React.createElement(I18nextProvider, { i18n },
          React.createElement(MemoryRouter, null,
            React.createElement(PreviewProvider as any, { persistNamespace: 'probe', subscribeGlobalOpen: false, workspacePath: 'D:/ws' },
              React.createElement(ThemeContext.Provider, { value: { theme: 'light', themePreference: 'light', setThemePreference: async () => {}, colorScheme: 'default', setColorScheme: async () => {}, fontScale: 1, setFontScale: async () => {} } as any },
                React.createElement(CloudAuthContext.Provider, { value: { ready: false, status: 'unauthenticated' } as any },
                  React.createElement(CreditsContext.Provider, { value: { checkInReceipt: null, checkInFailed: false, balance: 0, authenticated: false, balanceConfirmed: false, lastCheckInDayKey: 0, isFetchingBalance: false, isCheckingIn: false, lastRefreshAt: 0, cooldownSeconds: 0, canRefresh: false, fetchBalance: async () => {}, checkIn: async () => false, manualRefresh: () => {} } },
                    React.createElement(NomiChat as any, {
                      conversation_id: CONVERSATION_ID,
                      workspace: 'D:/ws',
                      isVoiceConversation: true,
                      modelSelection,
                    }))))))));
      await Promise.resolve();
    });

    // The actual composer accepts a draft; clicking send starts the real
    // dispatch path, whose host request is deliberately left pending.
    const suggestion = container.querySelector('button');
    expect(suggestion).toBeTruthy();
    await act(async () => {
      (suggestion as HTMLButtonElement).click();
      await Promise.resolve();
    });
    const send = container.querySelector('[data-testid="sendbox-send-btn"]') as HTMLButtonElement | null;
    expect(send).toBeTruthy();
    const composer = container.querySelector('[data-testid="sendbox-input"]') as HTMLElement | null;
    expect(composer).toBeTruthy();
    await act(async () => {
      composer!.dispatchEvent(new w.CompositionEvent('compositionstart', { bubbles: true }));
      composer!.dispatchEvent(new w.KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true, isComposing: true }));
      await Promise.resolve();
    });
    expect(sendStarted).toBe(false);
    await act(async () => {
      composer!.dispatchEvent(new w.CompositionEvent('compositionend', { bubbles: true }));
      composer!.dispatchEvent(new w.KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true }));
      await Promise.resolve();
    });

    expect(sendStarted).toBe(true);
    expect(container.textContent).toContain('正在发送');
    const waitRow = container.querySelector('[data-testid="voice-keyboard-wait-row"]');
    expect(waitRow).toBeTruthy();
    expect(
      waitRow?.closest('[data-testid="process-group"], [data-testid="turn-process-disclosure"]')
    ).toBeNull();
    // The first left row is the one that was published before the host await.
    expect(container.textContent).not.toContain('根因是等待状态没有先于请求发布。');

    // Virtual clock: after the measured 56s wait the same row is still present;
    // the absolute bound is covered by the pure helper test.
    const waitStartedAt = Date.now();
    const realDateNow = Date.now;
    let simulatedNow = waitStartedAt;
    Date.now = () => simulatedNow;
    try {
      simulatedNow = waitStartedAt + 56_000;
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 1000));
      });
      expect(container.textContent).toContain('正在发送');
      expect(waitRow?.textContent).toContain('56');
      simulatedNow = waitStartedAt + 57_000;
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 1000));
      });
      expect(waitRow?.textContent).toContain('57');
    } finally {
      Date.now = realDateNow;
    }

    // A real receipt moves the same local row to the accepted phase.
    await act(async () => {
      resolveSend?.({ task: sendTask });
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(container.textContent).toContain('任务已接单，正在执行');
    await act(async () => {
      resolveTaskGate?.();
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    // Completion reaches the view before the durable reply; refresh must
    // hand off to one authoritative left text row.
    taskStatus = 'completed';
    historyItems = [
      { message_id: '019b0000-0000-7000-8000-000000000912', conversation_id: CONVERSATION_ID, msg_id: '019b0000-0000-7000-8000-000000000912', type: 'text', content: { content: '分析这个项目的失败测试，修好后告诉我根因' }, position: 'right', status: 'finish', hidden: false, created_at: Date.now() },
      { message_id: '019b0000-0000-7000-8000-000000000913', conversation_id: CONVERSATION_ID, msg_id: '019b0000-0000-7000-8000-000000000913', type: 'text', content: { content: '根因是等待状态没有先于请求发布。' }, position: 'left', status: 'finish', hidden: false, created_at: Date.now() + 1 },
    ];
    const { emitter } = await import('@renderer/utils/emitter');
    await act(async () => {
      emitter.emit('conversation.messages.refresh', {
        conversationId: CONVERSATION_ID,
        reason: 'test',
      } as any);
      await new Promise((resolve) => setTimeout(resolve, 500));
    });
    expect(container.textContent).toContain('根因是等待状态没有先于请求发布。');
    expect(container.textContent?.match(/根因是等待状态没有先于请求发布。/g)?.length).toBe(1);
    expect(container.textContent).not.toContain('正在同步回复');
  } finally {
    await act(async () => root.unmount());
  }
}, 30000);
