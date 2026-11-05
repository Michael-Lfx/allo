// The pinned bun-types in this checkout predate the afterAll/afterEach
// declarations, while the runtime exports them (same stale-types gap as the
// existing `it` import in extractPresetModel.test.ts).
// @ts-expect-error bun:test types are pinned below the runtime version.
import {afterAll, afterEach, expect, test} from 'bun:test';
// jsdom ships its own runtime but no bundled TypeScript declarations here.
// @ts-expect-error jsdom has no bundled declarations and @types/jsdom is absent.
import {JSDOM} from 'jsdom';
import {createInstance} from 'i18next';
import type {VoiceTaskSnapshot} from '@common/adapter/voiceCompanion';
import zhVoiceJson from '@renderer/services/i18n/locales/zh-CN/voice.json';
import zhNomiJson from '@renderer/services/i18n/locales/zh-CN/nomi.json';

/**
 * VoicePetInput lives in the companion overlay, which has no DOM test host in
 * this repository (all other UI tests are pure or SSR). These tests install a
 * throwaway jsdom window so the real component, effects and timers can run.
 */
const dom = new JSDOM('<!doctype html><html><body></body></html>', {url: 'http://localhost/'});
const w = dom.window as unknown as Window & typeof globalThis;
const globals = globalThis as unknown as Record<string, unknown>;
const previousGlobals: Record<string, unknown> = {};
for (const key of ['window', 'document', 'navigator', 'HTMLElement', 'HTMLInputElement', 'Node', 'Event', 'KeyboardEvent', 'MouseEvent', 'sessionStorage', 'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame'] as const) {
  previousGlobals[key] = globals[key];
}
const restore = (key: string, value: unknown) => {
  if (value === undefined) delete globals[key];
  else globals[key] = value;
};
globals.window = w;
globals.document = w.document;
globals.navigator = w.navigator;
globals.HTMLElement = w.HTMLElement;
globals.HTMLInputElement = w.HTMLInputElement;
globals.Node = w.Node;
globals.Event = w.Event;
globals.KeyboardEvent = w.KeyboardEvent;
globals.MouseEvent = w.MouseEvent;
globals.sessionStorage = w.sessionStorage;
globals.getComputedStyle = w.getComputedStyle.bind(w);
globals.requestAnimationFrame = (callback: FrameRequestCallback) => setTimeout(() => callback(Date.now()), 0);
globals.cancelAnimationFrame = (handle: number) => clearTimeout(handle);
globals.IS_REACT_ACT_ENVIRONMENT = true;

afterAll(() => {
  for (const [key, value] of Object.entries(previousGlobals)) restore(key, value);
  delete globals.IS_REACT_ACT_ENVIRONMENT;
});

const zh = zhVoiceJson as unknown as Record<string, any>;
const zhNomi = zhNomiJson as unknown as Record<string, any>;
const conversationId = '019a1234-1234-7123-8123-123456789abc';
const otherConversationId = '019a1234-1234-7123-8123-123456789abd';
const draftKey = (id: string) => `pet-voice:${id}`;

const task = (overrides: Partial<VoiceTaskSnapshot> = {}): VoiceTaskSnapshot => ({
  command_id: 'cmd-1',
  session_id: 'session',
  acoustic_turn_id: 'turn',
  conversation_id: conversationId,
  message_id: null,
  runtime_turn_id: null,
  status: 'accepted',
  result_ok: null,
  result_text: null,
  error_code: null,
  ...overrides,
});

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return {promise, resolve, reject};
};

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

type Ports = {
  send?: (conversationId: string, commandId: string, text: string) => Promise<{task: VoiceTaskSnapshot}>;
  getTask?: (commandId: string) => Promise<VoiceTaskSnapshot>;
  openTask?: (conversationId: string) => Promise<void>;
  pollIntervalMs?: number;
  maxPollAttempts?: number;
};

type Harness = Awaited<ReturnType<typeof setup>>;

const setup = async (ports: Ports, conversation: string = conversationId) => {
  const React = await import('react');
  const {createRoot} = await import('react-dom/client');
  const {I18nextProvider} = await import('react-i18next');
  const {default: VoicePetInput} = await import('./VoicePetInput');
  const {useComposerDraftStore} = await import('@renderer/stores/composerDraftStore');
  const i18n = createInstance();
  await i18n.init({
    lng: 'zh-CN',
    resources: {'zh-CN': {translation: {voice: zh, nomi: zhNomi}}},
    initImmediate: false,
  });
  const act = (React as unknown as {act: (callback: () => Promise<void>) => Promise<void>}).act;
  const container = w.document.createElement('div');
  w.document.body.appendChild(container);
  const root = createRoot(container);
  const renderWith = async (id: string) => {
    await act(async () => {
      root.render(
        React.createElement(
          I18nextProvider,
          {i18n},
          React.createElement(VoicePetInput, {conversationId: id, workspace: 'D:/workspace', ports}),
        ),
      );
    });
  };
  await renderWith(conversation);
  const flush = async () => {
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  };
  const wait = async (ms: number) => {
    await act(async () => {
      await sleep(ms);
    });
  };
  const click = async (element: Element) => {
    await act(async () => {
      element.dispatchEvent(new w.MouseEvent('click', {bubbles: true}));
      await Promise.resolve();
    });
  };
  const pressEnter = async (element: HTMLInputElement, isComposing = false) => {
    await act(async () => {
      const event = new w.KeyboardEvent('keydown', {key: 'Enter', bubbles: true});
      Object.defineProperty(event, 'isComposing', {value: isComposing});
      element.dispatchEvent(event);
      await Promise.resolve();
    });
  };
  const button = () =>
    Array.from(container.querySelectorAll('button')).find(
      (candidate) => candidate.textContent === zhNomi.companion.send,
    ) as HTMLButtonElement;
  const input = () => container.querySelector('input') as HTMLInputElement;
  const status = () => container.querySelector('[role="status"]') as HTMLElement | null;
  const alert = () => container.querySelector('[role="alert"]') as HTMLElement | null;
  const setDraft = async (text: string, id: string = conversation) => {
    await act(async () => {
      useComposerDraftStore.getState().setDraft(draftKey(id), {text, files: [], dir: ''});
    });
  };
  const draftText = (id: string = conversation) => useComposerDraftStore.getState().drafts[draftKey(id)]?.text;
  const cleanup = async () => {
    await act(async () => {
      root.unmount();
    });
    container.remove();
  };
  const rerender = async (id: string) => {
    await renderWith(id);
    await flush();
  };
  return {React, act, container, root, i18n, flush, wait, click, pressEnter, button, input, status, alert, setDraft, draftText, cleanup, rerender};
};

let active: Harness | null = null;
afterEach(async () => {
  if (active) {
    await active.cleanup();
    active = null;
  }
  w.sessionStorage.clear();
  const {useComposerDraftStore} = await import('@renderer/stores/composerDraftStore');
  useComposerDraftStore.setState({drafts: {}});
});

const harness = async (ports: Ports, conversation: string = conversationId) => {
  active = await setup(ports, conversation);
  return active;
};

test('idle input exposes accessible labelling and no busy state', async () => {
  const app = await harness({});
  expect(app.input().getAttribute('aria-label')).toBe(zh.session.sendText);
  expect(app.input().getAttribute('placeholder')).toBe(zh.presentation.inputPlaceholder);
  expect(app.button().textContent).toBe(zhNomi.companion.send);
  expect(app.button().hasAttribute('aria-busy')).toBe(false);
  expect(app.status()).toBeNull();
  expect(app.alert()).toBeNull();
});

test('sending shows an accessible busy state immediately and clears the draft only after acceptance', async () => {
  const sent = deferred<{task: VoiceTaskSnapshot}>();
  const polledCommandIds: string[] = [];
  const app = await harness({
    send: async () => sent.promise,
    getTask: async (commandId) => {
      polledCommandIds.push(commandId);
      return task({command_id: commandId, status: 'queued'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('帮我查一下今天的学习计划');
  await app.flush();
  await app.click(app.button());

  // Busy is announced in addition to merely disabling the controls.
  expect(app.status()?.textContent).toBe(zh.task.status.delivering);
  expect(app.status()?.getAttribute('aria-live')).toBe('polite');
  expect(app.button().getAttribute('aria-busy')).toBe('true');
  expect(app.input().getAttribute('aria-busy')).toBe('true');
  expect(app.button().disabled).toBe(true);
  expect(app.status()?.textContent).not.toBe('');
  // The draft is still at risk while the POST has not been accepted.
  expect(app.draftText()).toBe('帮我查一下今天的学习计划');

  await app.act(async () => {
    sent.resolve({task: task({command_id: 'cmd-returned', status: 'queued'})});
    await Promise.resolve();
    await Promise.resolve();
  });
  expect(app.draftText()).toBeUndefined();
  await app.wait(30);
  expect(polledCommandIds[0]).toBe('cmd-returned');
  expect(app.container.textContent).toContain(zh.task.status.queued);
});

test('tracks the queried task status by its returned command id and stops at a terminal state', async () => {
  const sequence: VoiceTaskSnapshot['status'][] = ['queued', 'accepted', 'accepted', 'completed'];
  const polledIds: string[] = [];
  let polls = 0;
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-tracked', status: 'queued'})}),
    getTask: async (commandId) => {
      polledIds.push(commandId);
      const status = sequence[Math.min(polls, sequence.length - 1)];
      polls += 1;
      return task({command_id: commandId, status});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('生成一份周报');
  await app.flush();
  await app.click(app.button());
  await app.wait(80);

  expect(polledIds.every((commandId) => commandId === 'cmd-tracked')).toBe(true);
  expect(app.container.textContent).toContain(zh.task.status.completed);
  expect(polls).toBe(4);
  const settled = polls;
  await app.wait(40);
  expect(polls).toBe(settled);
});

test('a failed task shows the queried failure and never fabricates a thinking stream', async () => {
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-failed', status: 'accepted'})}),
    getTask: async (commandId) => task({command_id: commandId, status: 'failed', error_code: 'executor_error'}),
    pollIntervalMs: 5,
  });
  await app.setDraft('执行这个任务');
  await app.flush();
  await app.click(app.button());
  await app.wait(30);

  expect(app.container.textContent).toContain(zh.task.status.failed);
  expect(app.container.textContent).not.toContain('思考');
  expect(app.container.textContent).not.toContain('thinking');
  // Only authoritative status dictionaries are rendered, no invented transcript.
  expect(app.container.querySelectorAll('[role="status"]')).toHaveLength(1);
});

test('unknown receipt is reported then retried until the authoritative state arrives', async () => {
  let polls = 0;
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-unknown', status: 'delivering'})}),
    getTask: async (commandId) => {
      polls += 1;
      if (polls <= 3) throw new Error('offline');
      return task({command_id: commandId, status: 'accepted'});
    },
    pollIntervalMs: 20,
  });
  await app.setDraft('查询任务状态');
  await app.flush();
  await app.click(app.button());
  await app.wait(5);
  expect(app.container.textContent).toContain(zh.task.status.unknown);
  await app.wait(90);
  expect(app.container.textContent).toContain(zh.task.status.accepted);
});

test('never follows another task returned by the global voice query', async () => {
  let polls = 0;
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-own', status: 'accepted'})}),
    getTask: async () => {
      polls += 1;
      return task({command_id: 'cmd-foreign', conversation_id: otherConversationId, status: 'failed'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('只跟踪自己的任务');
  await app.flush();
  await app.click(app.button());
  await app.wait(30);

  expect(polls).toBe(1);
  expect(app.container.textContent).toContain(zh.task.status.accepted);
  expect(app.container.textContent).not.toContain(zh.task.status.failed);
});

test('a failed delivery keeps the draft and announces the failure', async () => {
  const app = await harness({
    send: async () => {
      throw new Error('network_lost');
    },
    getTask: async () => task(),
  });
  await app.setDraft('不要丢掉这段话');
  await app.flush();
  await app.click(app.button());
  await app.flush();

  expect(app.alert()?.textContent).toBe(zh.presentation.sendFailed);
  expect(app.draftText()).toBe('不要丢掉这段话');
  expect(app.input().value).toBe('不要丢掉这段话');
  expect(app.status()).toBeNull();
});

test('unmount stops the polling timer for the tracked task', async () => {
  let polls = 0;
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-unmount', status: 'accepted'})}),
    getTask: async (commandId) => {
      polls += 1;
      return task({command_id: commandId, status: 'accepted'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('轮询测试');
  await app.flush();
  await app.click(app.button());
  await app.wait(25);
  expect(polls).toBeGreaterThan(1);

  await app.cleanup();
  active = null;
  const beforeUnmount = polls;
  await sleep(30);
  expect(polls).toBe(beforeUnmount);
});

test('switching the frozen conversation stops polling the previous task and keeps drafts separate', async () => {
  let polls = 0;
  const app = await harness({
    send: async () => ({task: task({command_id: 'cmd-switch', status: 'accepted'})}),
    getTask: async (commandId) => {
      polls += 1;
      return task({command_id: commandId, status: 'accepted'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('旧会话输入');
  await app.flush();
  await app.click(app.button());
  await app.wait(20);
  expect(polls).toBeGreaterThan(0);
  await app.setDraft('新会话输入', otherConversationId);

  await app.rerender(otherConversationId);
  const frozen = polls;
  await app.wait(30);
  expect(polls).toBe(frozen);
  expect(app.input().value).toBe('新会话输入');
});

test('Enter submits once; IME composition and repeated Enter never double-submit', async () => {
  const sent = deferred<{task: VoiceTaskSnapshot}>();
  let sendCalls = 0;
  const app = await harness({
    send: async () => {
      sendCalls += 1;
      return sent.promise;
    },
    getTask: async (commandId) => task({command_id: commandId, status: 'accepted'}),
    pollIntervalMs: 5,
  });
  await app.setDraft('按回车发送');
  await app.flush();
  await app.pressEnter(app.input(), true);
  expect(sendCalls).toBe(0);
  await app.pressEnter(app.input(), false);
  expect(sendCalls).toBe(1);
  await app.pressEnter(app.input(), false);
  await app.pressEnter(app.input(), true);
  expect(sendCalls).toBe(1);
  await app.act(async () => {
    sent.resolve({task: task({status: 'accepted'})});
    await Promise.resolve();
  });
  await app.flush();
  expect(app.draftText()).toBeUndefined();
});

test('command ids stay scoped to the conversation that produced them', async () => {
  const seen: string[] = [];
  const app = await harness({
    send: async (_conversationId, commandId) => {
      seen.push(commandId);
      return {task: task({command_id: 'cmd-scoped', status: 'accepted'})};
    },
    getTask: async (commandId) => {
      seen.push(`poll:${commandId}`);
      return task({command_id: commandId, status: 'completed'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('作用域测试');
  await app.flush();
  await app.click(app.button());
  await app.wait(20);
  expect(seen[1]).toBe('poll:cmd-scoped');
});

test('resolves a persisted delivery with its original command identity instead of resending', async () => {
  const {uuidv7} = await import('@common/utils/uuidv7');
  const replayId = uuidv7();
  w.sessionStorage.setItem(
    `pet-voice:${conversationId}:delivery`,
    JSON.stringify({conversation_id: conversationId, input: '重放草稿', files: [], idempotency_key: replayId}),
  );
  const polled: string[] = [];
  let sends = 0;
  const app = await harness({
    send: async () => {
      sends += 1;
      throw new Error('must_not_resend');
    },
    getTask: async (commandId) => {
      polled.push(commandId);
      return task({command_id: commandId, status: 'accepted'});
    },
    pollIntervalMs: 5,
  });
  await app.setDraft('重放草稿');
  await app.click(app.button());
  await app.wait(20);

  expect(sends).toBe(0);
  expect(polled.length).toBeGreaterThan(0);
  expect(polled.every((commandId) => commandId === replayId)).toBe(true);
  expect(app.draftText()).toBeUndefined();
  expect(app.container.textContent).toContain(zh.task.status.accepted);
});
