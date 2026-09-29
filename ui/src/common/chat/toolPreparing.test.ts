import { describe, expect, test } from 'bun:test';
import { toolPreparingHintFromEvent } from './toolPreparing';

describe('toolPreparingHintFromEvent', () => {
  test('prefers a path field and marks it as a path', () => {
    expect(
      toolPreparingHintFromEvent({
        call_id: 'c1',
        name: 'Write',
        preview: { command: 'ignored', file_path: 'src/main.rs' },
      })
    ).toEqual({ callId: 'c1', tool: 'Write', target: { kind: 'path', value: 'src/main.rs' } });
  });

  test('keeps only the first line of a text target and bounds its length', () => {
    const hint = toolPreparingHintFromEvent({
      call_id: 'c2',
      name: 'exec_command',
      preview: { command: `cargo test ${'x'.repeat(200)}\necho done` },
    });
    expect(hint?.target?.kind).toBe('text');
    expect(hint?.target?.value.startsWith('cargo test ')).toBe(true);
    expect(hint?.target?.value.includes('echo done')).toBe(false);
    expect(hint?.target?.value.length).toBe(120);
  });

  test('a call without a known target still yields a hint', () => {
    expect(toolPreparingHintFromEvent({ call_id: 'c3', name: 'update_plan', preview: null })).toEqual({
      callId: 'c3',
      tool: 'update_plan',
    });
  });

  test('rejects payloads without identity', () => {
    expect(toolPreparingHintFromEvent(undefined)).toBeUndefined();
    expect(toolPreparingHintFromEvent({ call_id: '', name: 'Write' })).toBeUndefined();
    expect(toolPreparingHintFromEvent({ call_id: 'c4', name: '  ' })).toBeUndefined();
  });
});
