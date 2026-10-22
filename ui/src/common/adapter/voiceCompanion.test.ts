import { describe, expect, test } from 'bun:test';
import { createSnapshotConsumer, subscribeVoiceSession, stopVoiceSession, wakeVoiceSession, getVoiceDialogueSettings, voiceServiceAvailability, type VoiceSnapshot } from './voiceCompanion';
import { getVoiceFeedbackSettings, saveVoiceFeedback, previewVoiceFeedback } from './voiceCompanion';
import * as voice from './voiceCompanion';

test('explicit voice entry coalesces clicks, wakes once and opens only the frozen binding', async () => {
  const originalFetch = globalThis.fetch;
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const location = { hash: '' };
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { location } });
  const requests: string[] = [];
  let reads = 0;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input); requests.push(url);
    if (url.endsWith('/wake')) return Response.json({ success: true });
    reads += 1;
    return Response.json(reads === 1 ? { phase: 'idle' } : {
      phase: 'listening', session_id: 'current', binding: { session_id: 'current', conversation_id: 'frozen-target' },
    });
  }) as typeof fetch;
  try {
    const first = voice.startVoiceConversation();
    expect(voice.startVoiceConversation()).toBe(first);
    await first;
    expect(requests.filter(url => url.endsWith('/wake'))).toHaveLength(1);
    expect(location.hash).toBe('/conversation/frozen-target');
    expect(requests.some(url => url.includes('/tasks') || url.includes('/messages'))).toBe(false);
    requests.length = 0;
    await voice.startVoiceConversation();
    expect(requests).toHaveLength(1);
    expect(requests[0].endsWith('/session')).toBe(true);
  } finally {
    globalThis.fetch = originalFetch;
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});

test('tool decisions carry actual approval identities and never grant permanent permission',async()=>{
  const original=globalThis.fetch;const requests:Array<{url:string;init?:RequestInit}>=[];
  const task={command_id:'command',conversation_id:'dedicated-conversation',message_id:'accepted-message'} as voice.VoiceTaskSnapshot;
  const approval={approval_id:'actual-approval',msg_id:'actual-tool-message',call_id:'actual-call',tool_name:'Write',arguments:{file_path:'owned.txt'},expires_at_ms:Date.now()+8000};
  globalThis.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{requests.push({url:String(input),init});return Response.json(init?.method==='GET'?[approval]:{success:true});}) as typeof fetch;
  try {
    expect(await voice.getVoiceTaskConfirmations(task.command_id)).toEqual([approval]);
    await voice.confirmVoiceTask('host-instance',task,approval,true);
    expect(JSON.parse(requests[1].init?.body as string)).toEqual({instance_id:'host-instance',command_id:'command',conversation_id:'dedicated-conversation',message_id:'accepted-message',msg_id:'actual-tool-message',call_id:'actual-call',approved:true});
    expect(requests[1].url.endsWith('/tasks/command/confirmations/actual-approval')).toBe(true);
    expect(requests.every(({init})=>init?.credentials==='same-origin')).toBe(true);
  } finally {globalThis.fetch=original;}
});

test('task reads never write configuration or invoke wake or message delivery', async () => {
  const original = globalThis.fetch;
  const requests: Array<{url:string;init?:RequestInit}> = [];
  globalThis.fetch = (async (input:RequestInfo|URL,init?:RequestInit) => {
    requests.push({url:String(input),init}); return Response.json({status:'accepted'});
  }) as typeof fetch;
  try {
    await voice.getVoiceTask('owned-task');
    expect(requests).toHaveLength(1);
    expect(requests[0].url.endsWith('/tasks/owned-task')).toBe(true);
    expect(requests[0].init?.method).toBe('GET');
    expect(requests[0].init?.credentials).toBe('same-origin');
  } finally {globalThis.fetch=original;}
});

test('stopping voice does not cancel a task; explicit task cancellation carries the frozen task reference', async () => {
  const original=globalThis.fetch;
  const requests:Array<{url:string;init?:RequestInit}>=[];
  const task:voice.VoiceTaskSnapshot={command_id:'command',session_id:'original-session',acoustic_turn_id:'original-turn',
    conversation_id:'dedicated-conversation',message_id:'accepted-message',runtime_turn_id:'runtime-turn',status:'accepted',result_ok:null,result_text:null,error_code:null};
  const snapshot={instance_id:'host-instance',session_id:'new-voice-session',turn_id:'new-voice-turn',task} as VoiceSnapshot;
  globalThis.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{
    requests.push({url:String(input),init});return Response.json({...task,status:'cancelling'});
  }) as typeof fetch;
  try {
    await stopVoiceSession(snapshot);
    expect(requests).toHaveLength(1);
    expect(requests[0].url.endsWith('/api/voice/companion/session/stop')).toBe(true);
    const result=await voice.cancelVoiceTask(snapshot.instance_id,task);
    expect(result.status).toBe('cancelling');
    expect(requests[1].url.endsWith('/api/voice/companion/tasks/command/cancel')).toBe(true);
    expect(JSON.parse(requests[1].init?.body as string)).toEqual({instance_id:'host-instance',command_id:'command',conversation_id:'dedicated-conversation',message_id:'accepted-message'});
    expect(requests.every(({init})=>init?.credentials==='same-origin')).toBe(true);
  } finally {globalThis.fetch=original;}
});

test('sound preferences preserve omitted fields and preview never invokes wake or capture', async () => {
  const original = globalThis.fetch;
  const requests: Array<{url:string; init?:RequestInit}> = [];
  globalThis.fetch = (async (url:RequestInfo|URL, init?:RequestInit) => {
    requests.push({url:String(url),init});
    return Response.json({requested:{mode:'essential',volume:50,speak_replies:true},effective:null});
  }) as typeof fetch;
  try {
    await getVoiceFeedbackSettings();
    const saved = await saveVoiceFeedback({mode:'essential'});
    await previewVoiceFeedback('recognized');
    expect(saved.effective).toBeNull();
    expect(requests.map(r=>r.url.split('/api/')[1])).toEqual(['voice/companion/feedback','voice/companion/feedback','voice/companion/feedback/preview']);
    expect(JSON.parse(requests[1].init?.body as string)).toEqual({mode:'essential'});
    expect(JSON.parse(requests[2].init?.body as string)).toEqual({kind:'recognized'});
    expect(requests.every(r=>r.init?.credentials==='same-origin')).toBe(true);
  } finally { globalThis.fetch = original; }
});

test('all voice requests use same-origin cookies compatible with desktop wildcard CORS', async () => {
  const original = globalThis.fetch;
  const modes: Array<RequestCredentials | undefined> = [];
  const scope = { instance_id: 'host', seq: 1, session_id: 'session', turn_id: 'turn' } as VoiceSnapshot;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    modes.push(init?.credentials);
    if (String(input).endsWith('/events')) return new Response(new ReadableStream<Uint8Array>({ start(controller) {
      controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ type: 'Snapshot', payload: { snapshot: scope } })}\n\n`));
    } }));
    return Response.json(scope);
  }) as typeof fetch;
  let off: (() => void) | undefined;
  try {
    off = await subscribeVoiceSession(() => {}, false);
    await stopVoiceSession(scope);
    await wakeVoiceSession();
    await getVoiceDialogueSettings();
    expect(modes).toEqual(Array(5).fill('same-origin'));
  } finally { off?.(); globalThis.fetch = original; }
});

describe('voice snapshot handshake ordering', () => {
  test('missing service state is checking, never an assertion that the engine is unavailable', () => {
    expect(voiceServiceAvailability(null, true, null)).toBe('checking');
    expect(voiceServiceAvailability(null, true, 'voice_snapshot_unavailable')).toBe('syncFailed');
  });
  test('enabled intent and a running connection do not mean acoustic readiness', () => {
    const snapshot = { connection: 'connected', acoustic_ready: false } as VoiceSnapshot;
    expect(voiceServiceAvailability(snapshot, true, null)).toBe('acousticUnavailable');
    expect(voiceServiceAvailability({ ...snapshot, acoustic_ready: true }, true, null)).toBe('ready');
    expect(voiceServiceAvailability({ ...snapshot, connection: 'starting' }, true, null)).toBe('starting');
    expect(voiceServiceAvailability(snapshot, false, null)).toBe('disabled');
  });
  test('a late initial fetch cannot roll back a newer listening event', () => {
    const seen: VoiceSnapshot[] = [];
    const consume = createSnapshotConsumer((snapshot) => seen.push(snapshot));
    // Only identities participate in the ordering contract; no production fallback fixture.
    consume({ instance_id: 'host-a', seq: 3, phase: 'listening' } as VoiceSnapshot);
    consume({ instance_id: 'host-a', seq: 1, phase: 'idle' } as VoiceSnapshot);
    consume({ instance_id: 'host-a', seq: 3, phase: 'listening' } as VoiceSnapshot);
    expect(seen.map((snapshot) => snapshot.phase)).toEqual(['listening']);
  });
  test('retired hosts never overwrite the new host even with a higher sequence', () => {
    const seen: string[] = [];
    const consume = createSnapshotConsumer((snapshot) => seen.push(snapshot.instance_id));
    consume({ instance_id: 'host-a', seq: 100 } as VoiceSnapshot);
    consume({ instance_id: 'host-b', seq: 1 } as VoiceSnapshot);
    consume({ instance_id: 'host-a', seq: 101 } as VoiceSnapshot);
    expect(seen).toEqual(['host-a', 'host-b']);
  });
});

test('production stream subscription fences the slower snapshot fetch and releases its reader', async () => {
  const originalFetch = globalThis.fetch;
  const seen: number[] = [];
  let releaseFetch: (() => void) | undefined;
  let streamAborted = false;
  let readerCancelled = false;
  const newSnapshot = { instance_id: 'host-a', seq: 4, phase: 'listening' } as VoiceSnapshot;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).endsWith('/events')) {
      init?.signal?.addEventListener('abort', () => { streamAborted = true; });
      const body = new ReadableStream<Uint8Array>({ start(controller) {
        controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ type: 'Snapshot', payload: { snapshot: newSnapshot } })}\n\n`));
      }, cancel() { readerCancelled = true; } });
      return new Response(body, { headers: { 'Content-Type': 'text/event-stream' } });
    }
    await new Promise<void>((resolve) => { releaseFetch = resolve; });
    return Response.json({ ...newSnapshot, seq: 1, phase: 'idle' });
  }) as typeof fetch;
  try {
    const subscribing = subscribeVoiceSession((snapshot) => seen.push(snapshot.seq), false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(seen).toEqual([4]);
    releaseFetch?.();
    const off = await subscribing;
    expect(seen).toEqual([4]);
    off();
    expect(streamAborted).toBe(true);
    expect(readerCancelled).toBe(true);
  } finally { releaseFetch?.(); globalThis.fetch = originalFetch; }
});

test('stop sends the displayed scope and exposes a stale-scope rejection', async () => {
  const originalFetch = globalThis.fetch;
  let submitted: unknown;
  globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    submitted = JSON.parse(String(init?.body));
    return new Response(null, { status: 409 });
  }) as typeof fetch;
  try {
    const result = await stopVoiceSession({ instance_id: 'host-a', session_id: 'session-a', turn_id: 'turn-a' } as VoiceSnapshot)
      .then(() => 'unexpected_success', (error: Error) => error.message);
    expect(result).toBe('voice_stop_rejected');
    expect(submitted).toEqual({ instance_id: 'host-a', session_id: 'session-a', turn_id: 'turn-a' });
  } finally { globalThis.fetch = originalFetch; }
});

test('a failed initial GET does not destroy a healthy event subscription', async () => {
  const original = globalThis.fetch;
  let cancelled = false;
  globalThis.fetch = (async (input: RequestInfo | URL) => String(input).endsWith('/events')
    ? new Response(new ReadableStream<Uint8Array>({ start(controller) {
      controller.enqueue(new TextEncoder().encode('data: ' + JSON.stringify({ type: 'Snapshot', payload: { snapshot: { instance_id: 'host', seq: 1 } } }) + '\n\n'));
    }, cancel() { cancelled = true; } })) : new Response(null, { status: 503 })) as typeof fetch;
  let off: (() => void) | undefined;
  try {
    off = await subscribeVoiceSession(() => {}, false).catch(() => undefined);
    expect(typeof off).toBe('function');
    expect(cancelled).toBe(false);
  } finally { off?.(); globalThis.fetch = original; }
});

test('late initial GET from an old host never replaces the already received new host', async () => {
  const original = globalThis.fetch;
  let release: ((response: Response) => void) | undefined;
  const pending = new Promise<Response>((resolve) => { release = resolve; });
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined;
  const seen: string[] = [];
  const frame = (id: string, seq: number) => new TextEncoder().encode('data: ' + JSON.stringify({ type: 'Snapshot', payload: { snapshot: { instance_id: id, seq } } }) + '\n\n');
  globalThis.fetch = (async (input: RequestInfo | URL) => String(input).endsWith('/events')
    ? new Response(new ReadableStream<Uint8Array>({ start(next) { controller = next; next.enqueue(frame('new-host', 1)); } })) : pending) as typeof fetch;
  let off: (() => void) | undefined;
  try {
    const subscription = subscribeVoiceSession((snapshot) => seen.push(snapshot.instance_id), false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    release?.(Response.json({ instance_id: 'old-host', seq: 100 }));
    off = await subscription;
    controller?.enqueue(frame('new-host', 2));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(seen).toEqual(['new-host', 'new-host']);
  } finally { off?.(); globalThis.fetch = original; }
});

test('state sync failure clears when the live stream subsequently delivers readiness', async () => {
  const original = globalThis.fetch;
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined;
  const errors: Array<string | null> = [];
  const seen: boolean[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => String(input).endsWith('/events')
    ? new Response(new ReadableStream<Uint8Array>({ start(next) { controller = next; } })) : new Response(null, { status: 503 })) as typeof fetch;
  let off: (() => void) | undefined;
  try {
    off = await subscribeVoiceSession((snapshot) => seen.push(snapshot.acoustic_ready), false, (code) => errors.push(code));
    expect(errors[0]).toBe('voice_snapshot_unavailable');
    controller?.enqueue(new TextEncoder().encode('data: ' + JSON.stringify({ type: 'Snapshot', payload: { snapshot: { instance_id: 'host', seq: 1, acoustic_ready: true } } }) + '\n\n'));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(errors.at(-1)).toBe(null);
    expect(seen).toEqual([true]);
  } finally { off?.(); globalThis.fetch = original; }
});


test('target selection and replay use host-owned APIs and carry the current acoustic scope',async()=>{
  const original=globalThis.fetch;const requests:Array<{url:string;init?:RequestInit}>=[];
  globalThis.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{requests.push({url:String(input),init});return Response.json({success:true});}) as typeof fetch;
  try{
    await voice.getVoiceTargetSettings();await voice.saveVoiceTargetSettings({mode:'voice_home'});
    await voice.getVoiceNotification('accepted-command');
    const snapshot={instance_id:'host-now',session_id:'session-now',turn_id:'turn-now',task:{command_id:'accepted-command'}} as VoiceSnapshot;
    await voice.replayVoiceResult(snapshot,'one-explicit-replay');
    expect(requests.map(r=>r.url.split('/api/')[1])).toEqual(['voice/companion/target','voice/companion/target','voice/companion/tasks/accepted-command/notification','voice/companion/tasks/accepted-command/replay']);
    expect(JSON.parse(requests[3].init?.body as string)).toEqual({instance_id:'host-now',session_id:'session-now',turn_id:'turn-now',replay_id:'one-explicit-replay'});
    expect(requests.every(r=>r.init?.credentials==='same-origin')).toBe(true);
    expect(requests.some(r=>r.url.includes('/wake')||r.url.includes('/messages'))).toBe(false);
  }finally{globalThis.fetch=original;}
});
