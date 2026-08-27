import { describe, expect, test } from 'bun:test';
import type { MeetingSession } from '@/common/adapter/ipcBridge';
import { formatDurationMs, formatMs, groupSessionsByDate, notesPreview } from './format';

const session = (partial: Partial<MeetingSession> & { session_id: string }): MeetingSession => ({
  user_id: 'u',
  title: 'Meeting',
  status: 'stopped',
  bound_conversation_id: null,
  data_dir: '',
  mic_available: true,
  loopback_available: true,
  stt_backend: 'auto',
  started_at_ms: null,
  ended_at_ms: null,
  notes_status: 'none',
  notes: null,
  created_at_ms: 0,
  updated_at_ms: 0,
  ...partial,
});

describe('meeting format', () => {
  test('formats millisecond ranges as mm:ss', () => {
    expect(formatMs(0)).toBe('00:00');
    expect(formatMs(65_000)).toBe('01:05');
  });

  test('formats hour durations', () => {
    expect(formatDurationMs(3_661_000)).toBe('1:01:01');
    expect(formatDurationMs(65_000)).toBe('01:05');
  });

  test('groups sessions by local day buckets', () => {
    const now = Date.parse('2026-08-27T12:00:00');
    const groups = groupSessionsByDate(
      [
        session({ session_id: 'today', created_at_ms: Date.parse('2026-08-27T09:00:00') }),
        session({ session_id: 'yesterday', created_at_ms: Date.parse('2026-08-26T18:00:00') }),
        session({ session_id: 'week', created_at_ms: Date.parse('2026-08-24T10:00:00') }),
        session({ session_id: 'old', created_at_ms: Date.parse('2026-07-01T10:00:00') }),
      ],
      now
    );
    expect(groups.map((g) => [g.id, g.sessions.map((s) => s.session_id)])).toEqual([
      ['today', ['today']],
      ['yesterday', ['yesterday']],
      ['thisWeek', ['week']],
      ['earlier', ['old']],
    ]);
  });

  test('uses the first summary line as a card preview', () => {
    const preview = notesPreview(
      session({
        session_id: 'n',
        notes: {
          summary: 'Ship Friday.\nMore detail later.',
          decisions: [],
          todos: [],
          risks: [],
          speaker_highlights: [],
          source: 'llm',
          generated_at_ms: 1,
        },
      })
    );
    expect(preview).toBe('Ship Friday.');
  });
});
