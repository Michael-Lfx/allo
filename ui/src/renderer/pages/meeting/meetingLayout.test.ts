import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const detail = readFileSync(new URL('./MeetingDetailPage.tsx', import.meta.url), 'utf8');
const dock = readFileSync(new URL('./MeetingRecordingDock.tsx', import.meta.url), 'utf8');
const transcript = readFileSync(new URL('./MeetingTranscriptPanel.tsx', import.meta.url), 'utf8');
const list = readFileSync(new URL('./MeetingPage.tsx', import.meta.url), 'utf8');
const notes = readFileSync(new URL('./MeetingNotesPane.tsx', import.meta.url), 'utf8');

describe('meeting granola surface', () => {
  test('detail keeps the dock overlayed on a full-height notes scroller', () => {
    expect(detail.includes('meeting-cluster')).toBe(true);
    expect(detail.includes('meeting-detail-scroll')).toBe(true);
    expect(detail.includes('size-full')).toBe(true);
    expect(detail.includes('w-360px')).toBe(false);
    expect(detail.includes('meeting-back-link')).toBe(false);
  });

  test('dock is an idle mic pill and a recording stop control without pause', () => {
    expect(dock.includes('meeting-dock')).toBe(true);
    expect(dock.includes('meeting.dock.pause')).toBe(false);
    expect(dock.includes('formatDurationMs')).toBe(false);
    expect(dock.includes('meeting.dock.start')).toBe(true);
    expect(dock.includes("session.status === 'created'")).toBe(false);
  });

  test('transcript is a dark overlay without search or channel labels', () => {
    expect(transcript.includes('meeting-transcript-search')).toBe(false);
    expect(transcript.includes('meeting.channel.mic')).toBe(false);
    expect(transcript.includes('meeting.transcript.you')).toBe(true);
  });

  test('list rows are title plus date, not a control console', () => {
    expect(list.includes('<Tag')).toBe(false);
    expect(list.includes('notesPreview')).toBe(false);
    expect(list.includes('meeting-live-banner')).toBe(false);
  });

  test('notes pane is the note body, not an admin header', () => {
    expect(notes.includes('meeting.notes.title')).toBe(false);
    expect(notes.includes('<Tag')).toBe(false);
  });
});
