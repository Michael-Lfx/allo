import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dropdown, Menu } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Copy, Down, ExpandDown, ExpandUp, Loading, More } from '@icon-park/react';
import classNames from 'classnames';
import type { MeetingSegment, MeetingSessionStatus } from '@/common/adapter/ipcBridge';
import { formatMs, formatTranscriptText, isLiveStatus } from './format';

type MeetingTranscriptPanelProps = {
  expanded: boolean;
  segments: MeetingSegment[];
  loading: boolean;
  status: MeetingSessionStatus;
  onClose: () => void;
  onToggleExpanded: () => void;
};

const MeetingTranscriptPanel: React.FC<MeetingTranscriptPanelProps> = ({
  expanded,
  segments,
  loading,
  status,
  onClose,
  onToggleExpanded,
}) => {
  const { t } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const stuckToBottomRef = useRef(true);
  const prevLenRef = useRef(segments.length);
  const [contentVisible, setContentVisible] = useState(false);
  const live = isLiveStatus(status);

  useEffect(() => {
    const id = window.setTimeout(() => setContentVisible(true), 120);
    return () => window.clearTimeout(id);
  }, []);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const onScroll = () => {
      stuckToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
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
      await navigator.clipboard.writeText(formatTranscriptText(segments, t('meeting.transcript.you')));
      Message.success(t('meeting.copySuccess'));
    } catch {
      Message.error(t('meeting.copyFailed'));
    }
  };

  const speakerLabel = (segment: MeetingSegment): string => {
    if (segment.speaker_label.trim()) return segment.speaker_label;
    return segment.channel === 'mic' ? t('meeting.transcript.you') : t('meeting.transcript.them');
  };

  return (
    <aside className='meeting-transcript-panel flex h-full min-h-0 w-full flex-col overflow-hidden'>
      <div
        className={classNames(
          'flex items-center justify-between gap-12px px-16px py-10px transition-opacity',
          contentVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        <h2 className='m-0 min-w-0 truncate text-13px font-semibold text-white'>{t('meeting.transcript.title')}</h2>
        <div className='flex shrink-0 items-center gap-4px'>
          <button
            type='button'
            className='meeting-icon-btn'
            onClick={onToggleExpanded}
            aria-label={expanded ? t('meeting.transcript.collapse') : t('meeting.transcript.expand')}
          >
            {expanded ? (
              <ExpandDown theme='outline' size={16} fill='currentColor' />
            ) : (
              <ExpandUp theme='outline' size={16} fill='currentColor' />
            )}
          </button>
          <button
            type='button'
            className='meeting-icon-btn'
            onClick={onClose}
            aria-label={t('meeting.transcript.hide')}
          >
            <Down theme='outline' size={16} fill='currentColor' />
          </button>
          <Dropdown
            trigger='click'
            position='br'
            droplist={
              <Menu>
                <Menu.Item key='copy' disabled={segments.length === 0} onClick={() => void handleCopy()}>
                  <span className='inline-flex items-center gap-8px'>
                    <Copy theme='outline' size={14} />
                    {t('meeting.copyTranscript')}
                  </span>
                </Menu.Item>
              </Menu>
            }
          >
            <button
              type='button'
              className='meeting-icon-btn'
              disabled={segments.length === 0}
              aria-label={t('meeting.copyTranscript')}
            >
              <More theme='outline' size={16} fill='currentColor' />
            </button>
          </Dropdown>
        </div>
      </div>

      <div
        ref={scrollerRef}
        className={classNames(
          'min-h-0 flex-1 overflow-y-auto px-12px pt-12px pb-8px transition-opacity',
          contentVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        {loading ? (
          <div className='flex min-h-120px items-center justify-center text-13px text-white/50'>
          <span className='meeting-spin inline-flex'>
            <Loading theme='outline' size={16} fill='currentColor' />
          </span>
          </div>
        ) : segments.length === 0 ? (
          <div className='px-8px py-24px text-center text-13px text-white/60'>
            {live ? t('meeting.transcript.emptyLive') : t('meeting.transcript.emptyIdle')}
          </div>
        ) : (
          <ul className='m-0 flex list-none flex-col gap-8px p-0'>
            {segments.map((segment, index) => {
              const prev = index > 0 ? segments[index - 1] : null;
              const isYou = segment.channel === 'mic';
              const sameSpeaker = Boolean(prev && prev.speaker_label === segment.speaker_label && prev.channel === segment.channel);
              return (
                <li
                  key={segment.segment_id}
                  className={classNames(
                    'flex',
                    isYou ? 'justify-end' : 'justify-start',
                    sameSpeaker ? '' : 'mt-4px',
                    segment.is_partial && 'opacity-70'
                  )}
                >
                  <div className={classNames('max-w-[85%]', isYou ? 'items-end' : 'items-start')}>
                    {!sameSpeaker ? (
                      <div
                        className={classNames(
                          'mb-2px flex items-center gap-6px px-4px',
                          isYou ? 'flex-row-reverse' : 'flex-row'
                        )}
                      >
                        <span className={classNames('text-11px font-medium', isYou ? 'text-white/70' : 'text-[rgb(var(--success-5))]')}>
                          {speakerLabel(segment)}
                        </span>
                        <span className='text-10px tabular-nums text-white/40'>{formatMs(segment.start_ms)}</span>
                      </div>
                    ) : null}
                    <div
                      className={classNames(
                        'rounded-16px px-12px py-6px text-13px leading-20px',
                        isYou
                          ? 'rounded-br-6px bg-white text-[#171717]'
                          : 'rounded-bl-6px bg-white/15 text-white'
                      )}
                    >
                      {segment.text || '…'}
                    </div>
                  </div>
                </li>
              );
            })}
            <div ref={endRef} aria-hidden />
          </ul>
        )}
      </div>
      {live ? (
        <div className='flex shrink-0 items-center justify-center gap-8px px-16px py-12px text-white/70'>
          <span className='meeting-spin inline-flex'>
            <Loading theme='outline' size={14} fill='currentColor' />
          </span>
          {status !== 'stopping' ? (
            <span className='text-13px font-medium'>{t('meeting.transcript.listening')}</span>
          ) : null}
        </div>
      ) : null}
    </aside>
  );
};

export default MeetingTranscriptPanel;
