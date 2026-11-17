import * as bunTest from 'bun:test';
import { afterEach, describe, expect, test } from 'bun:test';
import { confirmVoiceModelRebuild, startVoiceModelSync } from './voiceModelSync';

// Bun exposes timers at runtime; the project's minimal bun:test declaration omits them.
const { jest } = bunTest as unknown as { jest: {
  useFakeTimers(): void;
  useRealTimers(): void;
  advanceTimersByTime(milliseconds: number): void;
} };

const originalFetch = globalThis.fetch;
let stop: (() => void) | undefined;
const flush = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
const tick = async (ms: number) => { jest.advanceTimersByTime(ms); await flush(); };
const json = (status: string) => Response.json({ state: { status } });
function harness(initial = 'Ready') {
  jest.useFakeTimers();
  let state = initial;
  const seen: string[] = [];
  const errors: boolean[] = [];
  const requests: string[] = [];
  const bodies: Record<string, unknown>[] = [];
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  let read: () => Promise<Response> = async () => json(state);
  let write: () => Promise<Response> = async () => Response.json({ accepted: true, state: { status: 'ProbingSource' } });
  globalThis.fetch = (async (url: string | URL | Request, init?: RequestInit) => {
    const path = String(url);
    requests.push(`${init?.method ?? 'GET'} ${path}`);
    if (typeof init?.body === 'string') bodies.push(JSON.parse(init.body));
    if (path.endsWith('/progress')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { streams.push(controller); } }));
    const response = path.endsWith('/status') ? read() : write();
    return Promise.race([response, new Promise<Response>((_, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
    })]);
  }) as typeof fetch;
  const sync = startVoiceModelSync(value => seen.push(value.status), error => errors.push(error));
  stop = sync.dispose;
  return {
    sync, seen, errors, requests, streams, bodies,
    state(value: string) { state = value; },
    read(value: () => Promise<Response>) { read = value; },
    write(value: () => Promise<Response>) { write = value; },
    event(value: string) { streams.at(-1)!.enqueue(new TextEncoder().encode(`data: {"status":"${value}"}\n\n`)); },
  };
}
afterEach(() => { stop?.(); stop = undefined; globalThis.fetch = originalFetch; jest.useRealTimers(); });

describe('voice settings model synchronization', () => {
  test('real LF and CRLF frames split across chunks refresh before the polling interval', async () => {
    const h = harness('Downloading');
    await flush();
    await tick(200);
    const reads = () => h.requests.filter(path => path.endsWith('/status')).length;
    const before = reads();
    h.state('Ready');
    const stream = h.streams.at(-1)!;
    const encoder = new TextEncoder();
    stream.enqueue(encoder.encode('data: {"status":"Re'));
    await flush();
    await tick(200);
    expect(reads()).toBe(before);
    stream.enqueue(encoder.encode('ady"}' + String.fromCharCode(10, 10)));
    await flush();
    await tick(200);
    expect(h.seen.at(-1)).toBe('Ready');
    expect(reads()).toBe(before + 1);
    h.state('Failed');
    stream.enqueue(encoder.encode('data: {"status":"Failed"}' + String.fromCharCode(13)));
    await flush();
    stream.enqueue(encoder.encode(String.fromCharCode(10, 13, 10)));
    await flush();
    await tick(200);
    expect(h.seen.at(-1)).toBe('Failed');
    expect(reads()).toBe(before + 2);
  });
  test('ambiguous KWS cancellation sends no POST; explicit confirmation adds only the rebuild field', async () => {
    const h = harness('Failed');
    await flush();
    const state = { status: 'Failed' as const, detail: { reason: 'kws_group_ambiguous: cannot identify group', recoverable: true } };
    const install = async (confirmed: boolean) => { await h.sync.operate('install', true, confirmed); };
    await confirmVoiceModelRebuild(state, async () => false, install);
    expect(h.bodies).toHaveLength(0);
    await confirmVoiceModelRebuild(state, async () => true, install);
    expect(h.bodies).toEqual([{ prefer_mirror: 'auto', force: true, rebuild_default_kws: true }]);
  });

  test('ordinary repair force never implies KWS rebuild confirmation', async () => {
    const h = harness();
    await flush();
    let confirmations = 0;
    await confirmVoiceModelRebuild({ status: 'Failed', detail: { reason: 'checksum mismatch', recoverable: true } },
      async () => { confirmations++; return true; },
      async (confirmed) => { await h.sync.operate('install', true, confirmed); });
    expect(confirmations).toBe(0);
    expect(h.bodies).toEqual([{ prefer_mirror: 'auto', force: true }]);
  });

  test('missed terminal events recover through polling without writes or microphone access', async () => {
    const h = harness('ProbingSource');
    await flush();
    h.state('Ready');
    await tick(3000);
    expect(h.seen.at(-1)).toBe('Ready');
    expect(h.requests.every(request => request.startsWith('GET ') && /models\/(status|progress)$/.test(request))).toBe(true);
  });

  test('stream reconnect reconciles after subscription and accepts changes from another page', async () => {
    const h = harness('Ready');
    await flush();
    h.streams[0].close();
    await flush();
    h.state('Removing');
    await tick(3000);
    expect(h.streams).toHaveLength(2);
    expect(h.seen.at(-1)).toBe('Removing');
    h.state('NotInstalled');
    h.event('NotInstalled');
    await flush();
    await tick(200);
    expect(h.seen.at(-1)).toBe('NotInstalled');
  });

  test('a late install receipt never rolls Ready back and pure verification reaches its terminal state', async () => {
    const h = harness('NotInstalled');
    await flush();
    let resolve!: (response: Response) => void;
    h.write(() => new Promise(done => { resolve = done; }));
    const pending = h.sync.operate('install', true);
    h.state('Ready');
    await tick(3000);
    expect(h.seen.at(-1)).toBe('Ready');
    resolve(Response.json({ accepted: true, state: { status: 'ProbingSource' } }));
    expect((await pending)?.ok).toBe(true);
    await flush();
    expect(h.seen.at(-1)).toBe('Ready');
    expect(h.seen).not.toContain('ProbingSource');
  });

  test('failed and malformed reads preserve state, then recovery clears only the sync error', async () => {
    const h = harness();
    await flush();
    h.read(async () => Response.json({ wrong: true }));
    await tick(3000);
    expect(h.seen.at(-1)).toBe('Ready');
    expect(h.errors.at(-1)).toBe(true);
    h.read(async () => json('Ready'));
    await tick(3000);
    expect(h.errors.at(-1)).toBe(false);
  });

  test('initial read errors never invent NotInstalled', async () => {
    const h = harness();
    h.read(async () => { throw new Error('offline'); });
    // A new page starts without a prior state.
    h.sync.dispose();
    h.seen.length = 0;
    const next = startVoiceModelSync(value => h.seen.push(value.status), error => h.errors.push(error));
    stop = next.dispose;
    await flush();
    expect(h.seen).toEqual([]);
    expect(h.errors.at(-1)).toBe(true);
  });

  test('a stalled read times out and a later poll recovers', async () => {
    const h = harness();
    await flush();
    h.read(() => new Promise(() => {}));
    await tick(3000);
    await tick(5000);
    expect(h.errors.at(-1)).toBe(true);
    expect(h.seen.at(-1)).toBe('Ready');
    h.read(async () => json('Failed'));
    await tick(3000);
    expect(h.seen.at(-1)).toBe('Failed');
    expect(h.errors.at(-1)).toBe(false);
  });

  test('progress bursts are coalesced and disposal cancels all further delivery and polling', async () => {
    const h = harness();
    await flush();
    await tick(200);
    const before = h.requests.length;
    for (let i = 0; i < 100; i++) h.event('ProbingSource');
    await flush();
    expect(h.requests.length).toBe(before);
    await tick(200);
    expect(h.requests.length - before).toBe(1);
    h.sync.dispose();
    const after = h.requests.length;
    const callbacks = h.seen.length + h.errors.length;
    await tick(60000);
    expect(h.requests.length).toBe(after);
    expect(h.seen.length + h.errors.length).toBe(callbacks);
  });

  test('unmount fences an in-flight write and read; old generations cannot publish', async () => {
    const h = harness();
    await flush();
    let resolve!: (response: Response) => void;
    h.read(() => new Promise(done => { resolve = done; }));
    await tick(3000);
    h.write(() => new Promise(() => {}));
    const pending = h.sync.operate('delete');
    h.sync.dispose();
    const count = h.seen.length;
    resolve(json('NotInstalled'));
    expect(await pending).toBeNull();
    await flush();
    expect(h.seen.length).toBe(count);
  });

  test('a pre-operation status response cannot overwrite the post-operation state', async () => {
    const h = harness();
    await flush();
    let resolve!: (response: Response) => void;
    h.read(() => new Promise(done => { resolve = done; }));
    await tick(3000);
    h.read(async () => json('NotInstalled'));
    h.write(async () => Response.json({ success: true, state: { status: 'NotInstalled' } }));
    await h.sync.operate('delete');
    await flush();
    await tick(200);
    expect(h.seen.at(-1)).toBe('NotInstalled');
    resolve(json('Ready'));
    await flush();
    expect(h.seen.at(-1)).toBe('NotInstalled');
  });

  test('a stalled operation is bounded and status recovery never issues another write', async () => {
    const h = harness();
    await flush();
    h.write(() => new Promise(() => {}));
    const pending = h.sync.operate('install');
    await tick(15000);
    expect((await pending)?.ok).toBe(false);
    await tick(3000);
    expect(h.requests.filter(request => request.startsWith('POST '))).toHaveLength(1);
    expect(h.seen.at(-1)).toBe('Ready');
  });

  test('a newer operation fences an older receipt and a silent stream is rotated', async () => {
    const h = harness();
    await flush();
    let resolve!: (response: Response) => void;
    h.write(() => new Promise(done => { resolve = done; }));
    const old = h.sync.operate('install');
    h.write(async () => Response.json({ success: true, state: { status: 'NotInstalled' } }));
    h.state('NotInstalled');
    expect((await h.sync.operate('delete'))?.ok).toBe(true);
    resolve(Response.json({ accepted: true, state: { status: 'ProbingSource' } }));
    expect(await old).toBeNull();
    await flush();
    await tick(200);
    expect(h.seen.at(-1)).toBe('NotInstalled');
    await tick(30000);
    await tick(3000);
    expect(h.streams.length).toBeGreaterThan(1);
  });

  test('both locales expose distinct status reading and status unavailable messages', async () => {
    const en = await import('../../services/i18n/locales/en-US/voice.json');
    const zh = await import('../../services/i18n/locales/zh-CN/voice.json');
    for (const locale of [en, zh]) {
      expect(locale.errors.kwsRebuildTitle.length).toBeGreaterThan(0);
      expect(locale.errors.kwsRebuildContent.length).toBeGreaterThan(0);
      expect(locale.errors.kwsRebuildConfirm.length).toBeGreaterThan(0);
      expect(locale.errors.modelStatusChecking.length).toBeGreaterThan(0);
      expect(locale.errors.modelStatusUnavailable.length).toBeGreaterThan(0);
      expect(locale.errors.modelStatusUnavailable).not.toBe(locale.errors.modelMissing);
    }
  });

  test('a stale SSE progress frame cannot roll back an authoritative terminal state', async () => {
    const h = harness();
    await flush();
    expect(h.seen.at(-1)).toBe('Ready');
    h.event('ProbingSource');
    await flush();
    expect(h.seen.at(-1)).toBe('Ready');
    await tick(200);
    expect(h.seen.at(-1)).toBe('Ready');
  });
});
