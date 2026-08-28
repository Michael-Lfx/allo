import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const detail = readFileSync(new URL('./MeetingDetailPage.tsx', import.meta.url), 'utf8');
const dock = readFileSync(new URL('./MeetingRecordingDock.tsx', import.meta.url), 'utf8');
const transcript = readFileSync(new URL('./MeetingTranscriptPanel.tsx', import.meta.url), 'utf8');
const list = readFileSync(new URL('./MeetingPage.tsx', import.meta.url), 'utf8');
const notes = readFileSync(new URL('./MeetingNotesPane.tsx', import.meta.url), 'utf8');

describe('meeting layout contract', () => {
  test('detail fills the content pane and keeps the dock outside the notes scroller', () => {
    expect(detail.includes("className='meeting-detail")).toBe(true);
    expect(detail.includes('size-full')).toBe(true);
    expect(detail.includes('meeting-cluster')).toBe(true);
    expect(detail.includes('meeting-detail-scroll')).toBe(true);
    expect(detail.includes('w-360px')).toBe(false);
    expect(detail.includes('w-520px')).toBe(false);
  });

  test('dock treats failed as idle and only shows start on created sessions', () => {
    expect(dock.includes('isLiveSession')).toBe(true);
    expect(dock.includes("session.status === 'failed'")).toBe(false);
    expect(dock.includes("session.status === 'created'")).toBe(true);
  });

  test('transcript overlay does not use a light Arco search field or channel labels', () => {
    expect(transcript.includes("from '@arco-design/web-react'")).toBe(false);
    expect(transcript.includes('meeting.channel.mic')).toBe(false);
    expect(transcript.includes('meeting-transcript-search')).toBe(true);
  });

  test('list rows are notes cards without status tags', () => {
    expect(list.includes('<Tag')).toBe(false);
    expect(list.includes('size-full')).toBe(true);
  });

  test('notes pane does not surface admin status tags', () => {
    expect(notes.includes('<Tag')).toBe(false);
    expect(notes.includes('meeting.notes.source')).toBe(false);
  });
});
