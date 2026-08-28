import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Copy, Down, Edit, ExpandDown, ExpandUp } from '@icon-park/react';
import classNames from 'classnames';
import type { MeetingSegment, MeetingSessionStatus } from '@/common/adapter/ipcBridge';
import { formatMs, formatTranscriptText, isLiveStatus } from './format';

type MeetingTranscriptPanelProps = {
  expanded: boolean;
  segments: MeetingSegment[];
  loading: boolean;
  status: MeetingSessionStatus;
  search: string;
  onSearchChange: (value: string) => void;
  onClose: () => void;
  onToggleExpanded: () => void;
  editingSegmentId: string | null;
  editingText: string;
  editSaving: boolean;
  onBeginEdit: (segment: MeetingSegment) => void;
  onEditingTextChange: (value: string) => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
};

const MeetingTranscriptPanel: React.FC<MeetingTranscriptPanelProps> = ({
  expanded,
  segments,
  loading,
  status,
  search,
  onSearchChange,
  onClose,
  onToggleExpanded,
  editingSegmentId,
  editingText,
  editSaving,
  onBeginEdit,
  onEditingTextChange,
  onSaveEdit,
  onCancelEdit,
}) => {
  const { t } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const stuckToBottomRef = useRef(true);
  const prevLenRef = useRef(segments.length);
  const [contentVisible, setContentVisible] = useState(false);
  const live = isLiveStatus(status);

  useEffect(() => {
    const id = window.setTimeout(() => setContentVisible(true), 80);
    return () => window.clearTimeout(id);
  }, []);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const onScroll = () => {
      stuckToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => {
    if (segments.length > prevLenRef.current && stuckToBottomRef.current) {
      endRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
    prevLenRef.current = segments.length;
  }, [segments.length]);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(formatTranscriptText(segments, t('meeting.speaker')));
      Message.success(t('meeting.copySuccess'));
    } catch {
      Message.error(t('meeting.copyFailed'));
    }
  };

  return (
    <aside className='meeting-transcript-panel flex h-full min-h-0 w-full flex-col overflow-hidden'>
      <div
        className={classNames(
          'flex items-center justify-between gap-8px px-14px py-10px transition-opacity',
          contentVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        <div className='min-w-0'>
          <div className='truncate text-13px font-semibold text-white'>{t('meeting.transcript.title')}</div>
          {live ? <div className='mt-2px text-11px text-white/55'>{t('meeting.transcript.listening')}</div> : null}
        </div>
        <div className='flex shrink-0 items-center gap-2px'>
          <button
            type='button'
            className='meeting-icon-btn'
            onClick={onToggleExpanded}
            aria-label={expanded ? t('meeting.transcript.collapse') : t('meeting.transcript.expand')}
          >
            {expanded ? (
              <ExpandDown theme='outline' size={14} fill='currentColor' />
            ) : (
              <ExpandUp theme='outline' size={14} fill='currentColor' />
            )}
          </button>
          <button
            type='button'
            className='meeting-icon-btn'
            onClick={onClose}
            aria-label={t('meeting.transcript.hide')}
          >
            <Down theme='outline' size={14} fill='currentColor' />
          </button>
          <button
            type='button'
            className='meeting-icon-btn'
            disabled={segments.length === 0}
            onClick={() => void handleCopy()}
            aria-label={t('meeting.copyTranscript')}
          >
            <Copy theme='outline' size={14} fill='currentColor' />
          </button>
        </div>
      </div>

      <div className='px-12px pb-8px'>
        <input
          className='meeting-transcript-search'
          value={search}
          onChange={(event) => onSearchChange(event.target.value)}
          placeholder={t('meeting.transcript.searchPlaceholder')}
        />
      </div>

      <div ref={scrollerRef} className='min-h-0 flex-1 overflow-y-auto px-12px pb-16px'>
        {loading ? (
          <div className='flex min-h-160px items-center justify-center text-13px text-white/50'>
            {t('meeting.transcript.listening')}
          </div>
        ) : segments.length === 0 ? (
          <div className='px-8px py-28px text-center text-13px text-white/50'>
            {live ? t('meeting.transcript.emptyLive') : t('meeting.transcript.emptyIdle')}
          </div>
        ) : (
          <ul className='m-0 flex list-none flex-col gap-10px p-0'>
            {segments.map((segment, index) => {
              const prev = index > 0 ? segments[index - 1] : null;
              const sameSpeaker = Boolean(prev && prev.speaker_label === segment.speaker_label);
              const isMic = segment.channel === 'mic';
              return (
                <li
                  key={segment.segment_id}
                  className={classNames(sameSpeaker ? 'mt-0' : 'mt-4px', segment.is_partial && 'opacity-70')}
                >
                  {!sameSpeaker ? (
                    <div className='mb-4px flex items-center gap-8px px-4px text-11px text-white/45'>
                      <span className={classNames('font-medium', isMic ? 'text-white/75' : 'text-[rgb(var(--success-5))]')}>
                        {segment.speaker_label || t('meeting.speaker')}
                      </span>
                      <span className='tabular-nums'>{formatMs(segment.start_ms)}</span>
                      {segment.is_manual_edit ? <span>{t('meeting.editedLabel')}</span> : null}
                    </div>
                  ) : null}
                  {editingSegmentId === segment.segment_id ? (
                    <div className='flex flex-col gap-8px'>
                      <textarea
                        className='meeting-transcript-edit'
                        rows={3}
                        value={editingText}
                        onChange={(event) => onEditingTextChange(event.target.value)}
                        placeholder={t('meeting.editPlaceholder')}
                      />
                      <div className='flex gap-8px'>
                        <button
                          type='button'
                          className='meeting-transcript-text-btn meeting-transcript-text-btn--primary'
                          disabled={editSaving}
                          onClick={onSaveEdit}
                        >
                          {t('meeting.saveEdit')}
                        </button>
                        <button
                          type='button'
                          className='meeting-transcript-text-btn'
                          disabled={editSaving}
                          onClick={onCancelEdit}
                        >
                          {t('meeting.cancelEdit')}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div
                      className={classNames(
                        'group relative max-w-[92%] rounded-16px px-12px py-8px text-13px leading-20px',
                        isMic
                          ? 'ml-auto rounded-br-6px bg-white text-[#111]'
                          : 'rounded-bl-6px bg-white/12 text-white'
                      )}
                    >
                      <div className='whitespace-pre-wrap'>{segment.text || '…'}</div>
                      <button
                        type='button'
                        className='absolute -right-2px -top-2px flex h-22px w-22px items-center justify-center rounded-full bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100'
                        onClick={() => onBeginEdit(segment)}
                        aria-label={t('meeting.edit')}
                      >
                        <Edit theme='outline' size={12} fill='currentColor' />
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
            <div ref={endRef} aria-hidden />
          </ul>
        )}
      </div>
    </aside>
  );
};

export default MeetingTranscriptPanel;
