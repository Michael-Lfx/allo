import { readFileSync } from 'node:fs';
import { expect, test } from 'bun:test';

declare const Bun: any;

function makeStopHandler(currentSession: { current: string | null }) {
  const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
  const button = source.indexOf("className='nomi-companion-voice-stop'");
  const marker = 'onClick={(event) => {';
  const start = source.indexOf(marker, button) + marker.length;
  const end = source.indexOf('\n                }}', start);
  if (button < 0 || start < marker.length || end < 0) throw new Error('voice stop handler not found');
  const code = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    `function create(env: any) { const { voiceSnapshot, stopVoiceSession, activeVoiceScopeRef, setVoiceStopFailed, setVoiceEntryFailed, document } = env; return (event: any) => { ${source.slice(start, end)}\n }; }`,
  );
  const factory = new Function(`${code}\nreturn create;`)();
  let rejectStop!: (error: Error) => void;
  const pending = new Promise<void>((_resolve, reject) => { rejectStop = reject; });
  const errors: boolean[] = [];
  const handler = factory({
    voiceSnapshot: { instance_id: 'host-a', session_id: 'A' }, stopVoiceSession: () => pending,
    activeVoiceScopeRef: currentSession, setVoiceStopFailed: (value: boolean) => errors.push(value),
    setVoiceEntryFailed: (value: boolean) => errors.push(value), document: { querySelector: () => null },
  });
  return { handler, rejectStop, errors };
}

for (const next of [null, 'host-a:B', 'host-b:A'] as const) {
  test(`late stop failure from A does not affect ${next ?? 'an ended call'}`, async () => {
    const current = { current: 'host-a:A' as string | null };
    const { handler, rejectStop, errors } = makeStopHandler(current);
    handler({ stopPropagation() {} });
    current.current = next;
    rejectStop(new Error('stop failed'));
    await Promise.resolve();
    expect(errors).not.toContain(true);
  });
}

test('a failed stop on the still-current call remains visible', async () => {
  const current = { current: 'host-a:A' as string | null };
  const { handler, rejectStop, errors } = makeStopHandler(current);
  handler({ stopPropagation() {} });
  rejectStop(new Error('stop failed'));
  await Promise.resolve();
  expect(errors).toContain(true);
});

test('an old entry failure cannot restore the unavailable banner after a newer call', async () => {
  const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
  const marker = 'const enterVoice = useCallback(async () => {';
  const start = source.indexOf(marker) + marker.length;
  const end = source.indexOf('\n  }, [voiceActive, voiceSnapshot, activeLocale]);', start);
  if (start < marker.length || end < 0) throw new Error('voice entry handler not found');
  const fields = ['setVoiceEntryFailed', 'voiceSessionRevisionRef', 'voiceActiveRef', 'voiceActive', 'voiceSnapshot', 'openVoiceTask', 'startVoiceConversation', 'activeLocale'];
  const code = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    `function create(env: any) { const { ${fields.join(', ')} } = env; return async () => { ${source.slice(start, end)}\n }; }`,
  );
  const factory = new Function(`${code}\nreturn create;`)();
  const revision = { current: 1 };
  const errors: boolean[] = [];
  let rejectStart!: (error: Error) => void;
  const startPromise = new Promise<void>((_resolve, reject) => { rejectStart = reject; });
  const handler = factory({
    setVoiceEntryFailed: (value: boolean) => errors.push(value), voiceSessionRevisionRef: revision,
    voiceActiveRef: { current: false }, voiceActive: false, voiceSnapshot: null,
    openVoiceTask: async () => {}, startVoiceConversation: () => startPromise, activeLocale: 'en-US',
  });
  const call = handler();
  revision.current = 3; // A later call started and ended while this request was pending.
  rejectStart(new Error('old entry failed'));
  await call;
  expect(errors).not.toContain(true);
});
