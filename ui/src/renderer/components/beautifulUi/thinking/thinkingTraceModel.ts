import type {
  ThinkingTraceItem,
  ThinkingTraceItemState,
  ThinkingTraceStatus,
  ThinkingTraceVariant,
} from './ThinkingTrace';

export type ThinkingTraceProcessState = 'running' | 'waiting' | 'completed' | 'failed' | 'canceled';

const CODE_EXT = 'ts|tsx|js|jsx|mjs|cjs|rs|py|go|java|kt|css|scss|json|toml|vue|svelte';
const FILE_ACTION = new RegExp(
  `^(?:Read|Edit|Write|Create|读取|编辑|写入)\\s+\\S+\\.(?:${CODE_EXT})\\b`,
  'i'
);
const DOC_TITLE = /\.(pdf|csv|xlsx|xls|docx|html)\s*$/i;
const NUMBERED_LINE = /^(?:\d+[\.\)、]|[一二三四五六七八九十]+、)\s+/;
const BULLET_LINE = /^[-*•]\s+/;

const parseTitleDetail = (chunk: string): { title: string; detail?: string } => {
  const lines = chunk.split('\n').map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) return { title: '' };
  if (lines.length === 1) return { title: lines[0] };
  return { title: lines[0], detail: lines.slice(1).join('\n') };
};

const collectMarkerStarts = (lines: string[], marker: RegExp): number[] =>
  lines.reduce<number[]>((starts, line, index) => {
    if (marker.test(line.trim())) starts.push(index);
    return starts;
  }, []);

const splitByMarkers = (
  text: string,
  marker: RegExp,
  stripMarker: boolean
): { title: string; detail?: string }[] => {
  const lines = text.split('\n');
  const starts = collectMarkerStarts(lines, marker);
  if (starts.length < 2) return [];
  return starts.map((start, index) => {
    const end = starts[index + 1] ?? lines.length;
    const raw = lines.slice(start, end).join('\n').trim();
    const chunk = stripMarker ? raw.replace(marker, '') : raw;
    return parseTitleDetail(chunk);
  });
};

const looksLikeCoding = (text: string): boolean => {
  if (text.includes('```')) return true;
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
  return lines.filter((line) => FILE_ACTION.test(line)).length >= 2;
};

const looksLikeSearch = (text: string): boolean => {
  if (/\b(searching|web search|search for)\b/i.test(text) || /搜索|检索/.test(text)) return true;
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
  return lines.filter((line) => DOC_TITLE.test(line)).length >= 2;
};

const looksLikeSteps = (text: string): boolean => {
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
  return collectMarkerStarts(lines, NUMBERED_LINE).length >= 2 || collectMarkerStarts(lines, BULLET_LINE).length >= 2;
};

export const inferThinkingTraceVariant = (text: string, subject?: string): ThinkingTraceVariant => {
  const hint = subject?.trim().toLowerCase() ?? '';
  if (hint) {
    if (/(^|\b)(code|coding|implement|refactor)(\b|$)/.test(hint) || hint.includes('编码')) return 'coding';
    if (/(^|\b)(search|retriev|web)(\b|$)/.test(hint) || /搜索|检索/.test(subject ?? '')) return 'search';
    if (/(^|\b)steps?(\b|$)/.test(hint) || (subject ?? '').includes('步骤')) return 'steps';
    if (/(^|\b)(reason|think|thought)(\b|$)/.test(hint)) return 'reasoning';
  }
  if (looksLikeCoding(text)) return 'coding';
  if (looksLikeSearch(text)) return 'search';
  if (looksLikeSteps(text)) return 'steps';
  return 'reasoning';
};

const splitThinkingSegments = (text: string): { title: string; detail?: string }[] => {
  const trimmed = text.trim().replace(/\n{3,}/g, '\n\n');
  if (!trimmed) return [];
  const numbered = splitByMarkers(trimmed, NUMBERED_LINE, true);
  if (numbered.length >= 2) return numbered;
  const bullets = splitByMarkers(trimmed, BULLET_LINE, true);
  if (bullets.length >= 2) return bullets;
  const fileActions = splitByMarkers(trimmed, FILE_ACTION, false);
  if (fileActions.length >= 2) return fileActions;
  const documents = splitByMarkers(trimmed, DOC_TITLE, false);
  if (documents.length >= 2) return documents;
  return [{ title: '', detail: trimmed }];
};

const itemStateFor = (index: number, lastIndex: number, status: ThinkingTraceStatus): ThinkingTraceItemState => {
  switch (status) {
    case 'thinking':
      return index === lastIndex ? 'running' : 'done';
    case 'waiting':
      return index === lastIndex ? 'pending' : 'done';
    case 'done':
    case 'failed':
    case 'canceled':
      return 'done';
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

export const buildThinkingTraceItems = (text: string, status: ThinkingTraceStatus): ThinkingTraceItem[] => {
  const segments = splitThinkingSegments(text).filter(
    (segment) => segment.title.trim().length > 0 || Boolean(segment.detail?.trim())
  );
  const lastIndex = segments.length - 1;
  return segments.map((segment, index) => ({
    id: String(index),
    title: segment.title,
    ...(segment.detail ? { detail: segment.detail } : {}),
    state: itemStateFor(index, lastIndex, status),
  }));
};

export const resolveThinkingTraceStatus = ({
  messageStatus,
  completed,
  forceDone,
  processState,
}: {
  messageStatus: 'thinking' | 'done';
  completed?: boolean;
  forceDone?: boolean;
  processState?: ThinkingTraceProcessState;
}): ThinkingTraceStatus => {
  if (processState) {
    switch (processState) {
      case 'running':
        return 'thinking';
      case 'waiting':
        return 'waiting';
      case 'completed':
        return 'done';
      case 'failed':
        return 'failed';
      case 'canceled':
        return 'canceled';
      default: {
        const exhaustive: never = processState;
        return exhaustive;
      }
    }
  }
  if (completed === true || forceDone === true || messageStatus === 'done') return 'done';
  return 'thinking';
};

export const isThinkingTraceSettled = (status: ThinkingTraceStatus): boolean => {
  switch (status) {
    case 'done':
    case 'failed':
    case 'canceled':
      return true;
    case 'thinking':
    case 'waiting':
      return false;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

export const isLiveProcessThinkingWindow = (
  layout: 'standalone' | 'process' | undefined,
  status: ThinkingTraceStatus
): boolean => layout === 'process' && !isThinkingTraceSettled(status);

export const pinScrollableToLatest = (element: {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}): void => {
  element.scrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
};
