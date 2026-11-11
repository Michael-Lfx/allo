import { describe, expect, test } from 'bun:test';
import { parseVoiceModelOperation, parseVoiceModelProgressLine } from './voiceCompanion';

describe('voice model operation receipts', () => {
  test('keeps the backend 409 in-use reason instead of replacing it with a generic failure', () => {
    const outcome = parseVoiceModelOperation(
      'install',
      409,
      JSON.stringify({
        error: 'voice_suite_in_use: finish the active voice call before installing models',
        code: 'voice_suite_in_use',
      }),
    );
    expect(outcome.ok).toBe(false);
    expect(outcome.code).toBe('voice_suite_in_use');
    expect(outcome.error).toContain('active voice call');
    expect(outcome.connectProgressStream).toBe(false);
  });

  test('a 200 delete with success=false keeps the real Ready state and exposes the reason', () => {
    const outcome = parseVoiceModelOperation(
      'delete',
      200,
      JSON.stringify({
        success: false,
        freed_bytes: 0,
        state: { status: 'Ready' },
        message: 'Removal incomplete: 1 expected asset(s) still present',
      }),
    );
    expect(outcome.ok).toBe(true);
    expect(outcome.receipt?.success).toBe(false);
    expect(outcome.receipt?.state?.status).toBe('Ready');
    expect(outcome.receipt?.message).toContain('Removal incomplete');
    expect(outcome.connectProgressStream).toBe(false);
  });

  test('an accepted=false install renders the returned state without forcing a new stream', () => {
    const outcome = parseVoiceModelOperation(
      'install',
      200,
      JSON.stringify({ accepted: false, state: { status: 'Ready' }, message: 'Models already installed' }),
    );
    expect(outcome.ok).toBe(true);
    expect(outcome.receipt?.accepted).toBe(false);
    expect(outcome.receipt?.state?.status).toBe('Ready');
    expect(outcome.connectProgressStream).toBe(false);
  });

  test('an accepted install subscribes to progress and keeps the backend state', () => {
    const outcome = parseVoiceModelOperation(
      'install',
      200,
      JSON.stringify({ accepted: true, state: { status: 'ProbingSource' } }),
    );
    expect(outcome.ok).toBe(true);
    expect(outcome.connectProgressStream).toBe(true);
    expect(outcome.receipt?.state?.status).toBe('ProbingSource');
  });

  test('an SSE progress line carries the authoritative install state', () => {
    const wrapped = parseVoiceModelProgressLine(
      'data: {"event":"progress","data":{"status":"Downloading","detail":{"current_file":"asr/model.int8.onnx","file_index":5,"total_files":7,"downloaded_bytes":100,"total_bytes":200,"progress_percent":70,"speed_bytes_per_sec":10}}}',
    );
    expect(wrapped?.status).toBe('Downloading');
    expect(wrapped?.detail).toBeDefined();
    expect(parseVoiceModelProgressLine('data: {"status":"Ready"}')?.status).toBe('Ready');
    expect(parseVoiceModelProgressLine('data: not-json')).toBeNull();
    expect(parseVoiceModelProgressLine(': keep-alive')).toBeNull();
  });

  test('a successful delete returns NotInstalled and does not report an error', () => {
    const outcome = parseVoiceModelOperation(
      'delete',
      200,
      JSON.stringify({ success: true, freed_bytes: 1024, state: { status: 'NotInstalled' } }),
    );
    expect(outcome.ok).toBe(true);
    expect(outcome.receipt?.success).toBe(true);
    expect(outcome.receipt?.state?.status).toBe('NotInstalled');
    expect(outcome.error).toBeUndefined();
  });


  test('malformed 2xx bodies fail closed instead of pretending success', () => {
    for (const body of ['', '<html>ok</html>', JSON.stringify({ accepted: true })]) {
      const outcome = parseVoiceModelOperation('install', 200, body);
      expect(outcome.ok).toBe(false);
      expect(outcome.receipt).toBeUndefined();
    }
  });
});
