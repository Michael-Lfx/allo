import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Dropdown, Input, Menu, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Copy, Down, Edit, ExpandDownOne, FoldUpOne } from '@icon-park/react';
import classNames from 'classnames';
import type { MeetingSegment, MeetingSessionStatus } from '@/common/adapter/ipcBridge';
import { formatMs, formatTranscriptText } from './format';

type MeetingTranscriptPanelProps = {
  open: boolean;
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

const channelLabelKey = (channel: MeetingSegment['channel']): string => {
  if (channel === 'mic') return 'meeting.channel.mic';
  if (channel === 'loopback') return 'meeting.channel.loopback';
  return 'meeting.channel.unknown';
};

const MeetingTranscriptPanel: React.FC<MeetingTranscriptPanelProps> = ({
  open,
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
  const [contentVisible, setContentVisible] = useState(open);

  const isLive = status === 'recording' || status === 'paused' || status === 'stopping';

  useEffect(() => {
    if (!open) {
      setContentVisible(false);
      return;
    }
    const id = window.setTimeout(() => setContentVisible(true), 80);
    return () => window.clearTimeout(id);
  }, [open]);

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

  const grouped = useMemo(() => segments, [segments]);

  if (!open) return null;

  return (
    <aside
      className={classNames(
        'flex h-full min-h-0 w-full shrink-0 flex-col overflow-hidden rounded-16px',
        'bg-[rgba(12,14,18,0.86)] text-white shadow-[0_8px_28px_rgba(0,0,0,0.28)] backdrop-blur-16px'
      )}
    >
      <div
        className={classNames(
          'flex items-center justify-between gap-8px px-14px py-10px transition-opacity',
          contentVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        <div className='min-w-0'>
          <div className='truncate text-13px font-semibold'>{t('meeting.transcript.title')}</div>
          {isLive ? (
            <div className='mt-2px text-11px text-white/55'>{t('meeting.transcript.listening')}</div>
          ) : null}
        </div>
        <div className='flex shrink-0 items-center gap-2px'>
          <Button
            type='text'
            size='mini'
            className='!text-white/70 hover:!bg-white/10 hover:!text-white'
            onClick={onToggleExpanded}
            aria-label={expanded ? t('meeting.transcript.collapse') : t('meeting.transcript.expand')}
          >
            {expanded ? (
              <FoldUpOne theme='outline' size={14} fill='currentColor' />
            ) : (
              <ExpandDownOne theme='outline' size={14} fill='currentColor' />
            )}
          </Button>
          <Button
            type='text'
            size='mini'
            className='!text-white/70 hover:!bg-white/10 hover:!text-white'
            onClick={onClose}
            aria-label={t('meeting.transcript.hide')}
          >
            <Down theme='outline' size={14} fill='currentColor' />
          </Button>
          <Dropdown
            droplist={
              <Menu>
                <Menu.Item key='copy' disabled={segments.length === 0} onClick={() => void handleCopy()}>
                  {t('meeting.copyTranscript')}
                </Menu.Item>
              </Menu>
            }
            trigger='click'
            position='br'
          >
            <Button
              type='text'
              size='mini'
              className='!text-white/70 hover:!bg-white/10 hover:!text-white'
              disabled={segments.length === 0}
              aria-label={t('meeting.copyTranscript')}
            >
              <Copy theme='outline' size={14} fill='currentColor' />
            </Button>
          </Dropdown>
        </div>
      </div>

      <div className='px-12px pb-8px'>
        <Input
          size='small'
          allowClear
          value={search}
          onChange={onSearchChange}
          placeholder={t('meeting.transcript.searchPlaceholder')}
        />
      </div>

      <div ref={scrollerRef} className='min-h-0 flex-1 overflow-y-auto px-12px pb-16px'>
        {loading ? (
          <div className='flex min-h-160px items-center justify-center'>
            <Spin />
          </div>
        ) : grouped.length === 0 ? (
          <div className='px-8px py-28px text-center text-13px text-white/50'>
            {isLive ? t('meeting.transcript.emptyLive') : t('meeting.transcript.emptyIdle')}
          </div>
        ) : (
          <ul className='m-0 flex list-none flex-col gap-10px p-0'>
            {grouped.map((segment, index) => {
              const prev = index > 0 ? grouped[index - 1] : null;
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
                      <span>{t(channelLabelKey(segment.channel))}</span>
                      {segment.is_manual_edit ? <span>{t('meeting.editedLabel')}</span> : null}
                    </div>
                  ) : null}
                  {editingSegmentId === segment.segment_id ? (
                    <div className='flex flex-col gap-8px'>
                      <Input.TextArea
                        autoSize={{ minRows: 2, maxRows: 8 }}
                        value={editingText}
                        onChange={onEditingTextChange}
                        placeholder={t('meeting.editPlaceholder')}
                      />
                      <div className='flex gap-8px'>
                        <Button size='mini' type='primary' loading={editSaving} onClick={onSaveEdit}>
                          {t('meeting.saveEdit')}
                        </Button>
                        <Button size='mini' disabled={editSaving} onClick={onCancelEdit}>
                          {t('meeting.cancelEdit')}
                        </Button>
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
                        className='absolute -right-2px -top-2px hidden h-22px w-22px items-center justify-center rounded-full bg-black/40 text-white group-hover:flex'
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
