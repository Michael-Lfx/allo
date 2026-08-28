import type { MeetingSegment, MeetingSession } from '@/common/adapter/ipcBridge';

export const formatMs = (ms: number): string => {
  const totalSec = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
};

export const formatDurationMs = (ms: number): string => {
  const totalSec = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  }
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
};

export const formatRelativeTime = (ms: number, locale: string, nowMs = Date.now()): string => {
  const diffSeconds = Math.floor((nowMs - ms) / 1000);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (diffSeconds < 60) return rtf.format(0, 'second');
  const diffMins = Math.floor(diffSeconds / 60);
  if (diffMins < 60) return rtf.format(-diffMins, 'minute');
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return rtf.format(-diffHours, 'hour');
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) return rtf.format(-diffDays, 'day');
  return new Intl.DateTimeFormat(locale, {
    month: 'short',
    day: 'numeric',
    year: new Date(ms).getFullYear() !== new Date(nowMs).getFullYear() ? 'numeric' : undefined,
  }).format(new Date(ms));
};

export const startOfLocalDay = (ms: number): number => {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};

export type MeetingDateGroupId = 'today' | 'yesterday' | 'thisWeek' | 'earlier';

export type MeetingDateGroup = {
  id: MeetingDateGroupId;
  sessions: MeetingSession[];
};

export const groupSessionsByDate = (
  sessions: MeetingSession[],
  nowMs = Date.now()
): MeetingDateGroup[] => {
  const today = startOfLocalDay(nowMs);
  const yesterday = today - 24 * 60 * 60 * 1000;
  const weekStart = today - 6 * 24 * 60 * 60 * 1000;
  const buckets: Record<MeetingDateGroupId, MeetingSession[]> = {
    today: [],
    yesterday: [],
    thisWeek: [],
    earlier: [],
  };

  for (const session of sessions) {
    const ts = session.started_at_ms ?? session.created_at_ms;
    const day = startOfLocalDay(ts);
    if (day >= today) buckets.today.push(session);
    else if (day >= yesterday) buckets.yesterday.push(session);
    else if (day >= weekStart) buckets.thisWeek.push(session);
    else buckets.earlier.push(session);
  }

  const order: MeetingDateGroupId[] = ['today', 'yesterday', 'thisWeek', 'earlier'];
  return order
    .filter((id) => buckets[id].length > 0)
    .map((id) => ({ id, sessions: buckets[id] }));
};

export const notesPreview = (session: MeetingSession): string => {
  const summary = session.notes?.summary?.trim();
  if (!summary) return '';
  const line = summary.split(/\n/)[0]?.trim() ?? '';
  return line.length > 120 ? `${line.slice(0, 117).trimEnd()}…` : line;
};

export const isLiveStatus = (status: MeetingSession['status']): boolean =>
  status === 'recording' || status === 'paused' || status === 'stopping';

export const isLiveSession = (session: MeetingSession): boolean => isLiveStatus(session.status);

export const filterSegments = (segments: MeetingSegment[], query: string): MeetingSegment[] => {
  const q = query.trim().toLowerCase();
  if (!q) return segments;
  return segments.filter(
    (segment) =>
      segment.text.toLowerCase().includes(q) || segment.speaker_label.toLowerCase().includes(q)
  );
};

export const defaultMeetingTitle = (locale: string, now = new Date()): string => {
  const date = now.toLocaleDateString(locale, { month: 'short', day: 'numeric' });
  const time = now.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
  return `${date} ${time}`;
};

export const formatTranscriptText = (
  segments: Array<{ speaker_label: string; start_ms: number; text: string }>,
  fallbackSpeaker: string
): string =>
  segments
    .map((segment) => {
      const speaker = segment.speaker_label.trim() || fallbackSpeaker;
      return `[${speaker} ${formatMs(segment.start_ms)}] ${segment.text}`;
    })
    .join('\n');

export const formatNotesMarkdown = (session: MeetingSession): string => {
  const notes = session.notes;
  if (!notes) return session.title;
  const lines = [`# ${session.title}`, ''];
  if (notes.summary.trim()) {
    lines.push(notes.summary.trim(), '');
  }
  if (notes.decisions.length > 0) {
    lines.push('## Decisions');
    for (const item of notes.decisions) lines.push(`- ${item}`);
    lines.push('');
  }
  if (notes.todos.length > 0) {
    lines.push('## Todos');
    for (const item of notes.todos) {
      lines.push(`- ${item.title}${item.detail ? ` — ${item.detail}` : ''}`);
    }
    lines.push('');
  }
  if (notes.risks.length > 0) {
    lines.push('## Risks');
    for (const item of notes.risks) lines.push(`- ${item}`);
    lines.push('');
  }
  if (notes.speaker_highlights.length > 0) {
    lines.push('## Highlights');
    for (const item of notes.speaker_highlights) {
      lines.push(`- ${item.speaker}: ${item.highlight}`);
    }
  }
  return lines.join('\n').trim();
};
