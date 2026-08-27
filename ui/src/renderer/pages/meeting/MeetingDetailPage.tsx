import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { Alert, Button, Empty, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Left } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import type { MeetingSegment } from '@/common/adapter/ipcBridge';
import MeetingAdvancedPanel from './MeetingAdvancedPanel';
import MeetingNotesPane from './MeetingNotesPane';
import MeetingRecordingDock from './MeetingRecordingDock';
import MeetingTranscriptPanel from './MeetingTranscriptPanel';
import VoiceprintModal from './VoiceprintModal';
import { formatRelativeTime, isLiveSession } from './format';
import { useMeetings } from './useMeetings';
import './meeting.css';

const MeetingDetailPage: React.FC = () => {
  const { sessionId = '' } = useParams<{ sessionId: string }>();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const {
    sessions,
    selected,
    segments,
    segmentsLoading,
    devices,
    voiceprints,
    capabilityDegrade,
    listenStatus,
    loading,
    lastError,
    setLastError,
    startSession,
    pauseSession,
    resumeSession,
    stopSession,
    bindConversation,
    updateTitle,
    enrollVoiceprint,
    deleteVoiceprint,
    generateNotes,
    editSegment,
    searchSegments,
    startListen,
    stopListen,
  } = useMeetings({ sessionId });

  const [title, setTitle] = useState('');
  const [actionBusy, setActionBusy] = useState(false);
  const [voiceprintOpen, setVoiceprintOpen] = useState(false);
  const [transcriptOpen, setTranscriptOpen] = useState(false);
  const [transcriptExpanded, setTranscriptExpanded] = useState(false);
  const [transcriptQuery, setTranscriptQuery] = useState('');
  const [editingSegmentId, setEditingSegmentId] = useState<string | null>(null);
  const [editingText, setEditingText] = useState('');
  const [editSaving, setEditSaving] = useState(false);
  const [nowMs, setNowMs] = useState(Date.now());
  const titleTimerRef = useRef<number | null>(null);
  const searchTimerRef = useRef<number | null>(null);
  const openedForLiveRef = useRef<string | null>(null);

  useEffect(() => {
    setTitle(selected?.title ?? '');
    setTranscriptQuery('');
    setEditingSegmentId(null);
  }, [selected?.session_id, selected?.title]);

  useEffect(() => {
    if (!selected) return;
    if (isLiveSession(selected) && openedForLiveRef.current !== selected.session_id) {
      openedForLiveRef.current = selected.session_id;
      setTranscriptOpen(true);
    }
  }, [selected]);

  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    if (!lastError) return;
    Message.error(t('meeting.error.event', { message: lastError }));
    setLastError(null);
  }, [lastError, setLastError, t]);

  useEffect(() => {
    return () => {
      if (titleTimerRef.current) window.clearTimeout(titleTimerRef.current);
      if (searchTimerRef.current) window.clearTimeout(searchTimerRef.current);
    };
  }, []);

  const elapsedMs = useMemo(() => {
    if (!selected?.started_at_ms) return 0;
    const end = selected.ended_at_ms ?? nowMs;
    return Math.max(0, end - selected.started_at_ms);
  }, [nowMs, selected?.ended_at_ms, selected?.started_at_ms]);

  const activeDegrade =
    capabilityDegrade && selected && capabilityDegrade.session_id === selected.session_id
      ? capabilityDegrade
      : null;

  const sessionCapabilityMessage = useMemo(() => {
    if (!selected) return null;
    if (selected.mic_available && selected.loopback_available) return null;
    if (!selected.mic_available && !selected.loopback_available) {
      return t('meeting.capability.bothMissing');
    }
    if (!selected.mic_available) return t('meeting.capability.micMissing');
    return t('meeting.capability.loopbackMissing');
  }, [selected, t]);

  const persistTitle = useCallback(
    (next: string) => {
      if (!selected) return;
      const trimmed = next.trim();
      if (!trimmed || trimmed === selected.title) return;
      if (titleTimerRef.current) window.clearTimeout(titleTimerRef.current);
      titleTimerRef.current = window.setTimeout(() => {
        void updateTitle(selected.session_id, trimmed).catch((err) => Message.error(String(err)));
      }, 500);
    },
    [selected, updateTitle]
  );

  const handleTitleChange = useCallback(
    (value: string) => {
      setTitle(value);
      persistTitle(value);
    },
    [persistTitle]
  );

  const run = useCallback(
    async (task: () => Promise<unknown>, successMessage?: string) => {
      setActionBusy(true);
      try {
        await task();
        if (successMessage) Message.success(successMessage);
      } catch (err) {
        Message.error(String(err));
      } finally {
        setActionBusy(false);
      }
    },
    []
  );

  const handleStart = useCallback(() => {
    if (!selected) return;
    setTranscriptOpen(true);
    void run(() => startSession(selected.session_id), t('meeting.startSuccess'));
  }, [run, selected, startSession, t]);

  const handlePause = useCallback(() => {
    if (!selected) return;
    void run(() => pauseSession(selected.session_id), t('meeting.pauseSuccess'));
  }, [pauseSession, run, selected, t]);

  const handleResume = useCallback(() => {
    if (!selected) return;
    void run(() => resumeSession(selected.session_id), t('meeting.resumeSuccess'));
  }, [resumeSession, run, selected, t]);

  const handleStop = useCallback(() => {
    if (!selected) return;
    void run(() => stopSession(selected.session_id), t('meeting.stopSuccess'));
  }, [run, selected, stopSession, t]);

  const handleGenerateNotes = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      const result = await generateNotes(selected.session_id);
      const parts = [t('meeting.notes.generateSuccess')];
      if (result.posted_to_conversation) parts.push(t('meeting.notes.posted'));
      if (result.created_requirement_ids.length > 0) {
        parts.push(t('meeting.notes.tasksCreated', { count: result.created_requirement_ids.length }));
      }
      Message.success(parts.join(' · '));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [generateNotes, selected, t]);

  const handleSearch = useCallback(
    (value: string) => {
      setTranscriptQuery(value);
      if (!selected) return;
      if (searchTimerRef.current) window.clearTimeout(searchTimerRef.current);
      searchTimerRef.current = window.setTimeout(() => {
        void searchSegments(selected.session_id, value);
      }, 220);
    },
    [searchSegments, selected]
  );

  const beginEditSegment = useCallback((segment: MeetingSegment) => {
    setEditingSegmentId(segment.segment_id);
    setEditingText(segment.text);
  }, []);

  const saveEditSegment = useCallback(async () => {
    if (!selected || !editingSegmentId) return;
    setEditSaving(true);
    try {
      await editSegment(selected.session_id, editingSegmentId, editingText);
      Message.success(t('meeting.editSuccess'));
      setEditingSegmentId(null);
      setEditingText('');
    } catch (err) {
      Message.error(String(err));
    } finally {
      setEditSaving(false);
    }
  }, [editSegment, editingSegmentId, editingText, selected, t]);

  const missing = !loading && sessionId && !sessions.some((item) => item.session_id === sessionId) && !selected;

  return (
    <div className='meeting-detail relative flex h-full min-h-0 w-full flex-1 overflow-hidden'>
      {loading && !selected ? (
        <div className='flex h-full items-center justify-center'>
          <Spin />
        </div>
      ) : missing ? (
        <div className='flex h-full flex-col items-center justify-center gap-12px'>
          <Empty description={t('meeting.notFound')} />
          <Button onClick={() => navigate('/meeting')}>{t('meeting.back')}</Button>
        </div>
      ) : selected ? (
        <div
          className={classNames(
            'flex h-full min-h-0',
            isMobile || !transcriptOpen ? 'flex-col' : 'flex-row'
          )}
        >
          <div className='relative min-h-0 min-w-0 flex-1 overflow-y-auto'>
            <div
              className={classNames(
                'mx-auto flex w-full max-w-760px flex-col pb-96px',
                isMobile ? 'px-16px pt-12px' : 'px-28px pt-20px'
              )}
            >
              <div className='mb-8px'>
                <Button type='text' size='small' icon={<Left theme='outline' size={14} />} onClick={() => navigate('/meeting')}>
                  {t('meeting.back')}
                </Button>
              </div>

              {(activeDegrade || sessionCapabilityMessage) && (
                <Alert
                  className='mb-12px'
                  type='warning'
                  content={
                    activeDegrade
                      ? t('meeting.capability.banner', { message: activeDegrade.message })
                      : sessionCapabilityMessage
                  }
                />
              )}

              <input
                className='meeting-title-input w-full bg-transparent text-t-primary'
                value={title}
                onChange={(event) => handleTitleChange(event.target.value)}
                placeholder={t('meeting.titlePlaceholder')}
              />

              <div className='mb-18px mt-6px flex flex-wrap items-center gap-8px text-13px text-t-tertiary'>
                <span>
                  {t('meeting.edited', {
                    date: formatRelativeTime(selected.updated_at_ms || selected.created_at_ms, i18n.language),
                  })}
                </span>
                <span aria-hidden>·</span>
                <span>{t(`meeting.status.${selected.status}`)}</span>
              </div>

              <MeetingNotesPane
                session={selected}
                generating={actionBusy || selected.notes_status === 'generating'}
                onGenerate={() => void handleGenerateNotes()}
              />

              <div className='mt-28px'>
                <MeetingAdvancedPanel
                  session={selected}
                  devices={devices}
                  voiceprints={voiceprints}
                  listenStatus={listenStatus}
                  busy={actionBusy}
                  onBind={(conversationId) => bindConversation(selected.session_id, conversationId)}
                  onListenToggle={async (enabled, conversationId) => {
                    if (enabled) {
                      await startListen(selected.session_id, conversationId || selected.bound_conversation_id);
                    } else {
                      await stopListen(selected.session_id);
                    }
                  }}
                  onOpenVoiceprint={() => setVoiceprintOpen(true)}
                />
              </div>
            </div>

            <MeetingRecordingDock
              session={selected}
              elapsedMs={elapsedMs}
              busy={actionBusy}
              transcriptOpen={transcriptOpen}
              onToggleTranscript={() => setTranscriptOpen((open) => !open)}
              onStart={handleStart}
              onPause={handlePause}
              onResume={handleResume}
              onStop={handleStop}
            />
          </div>

          {transcriptOpen ? (
            <div
              className={classNames(
                'min-h-0 p-12px',
                isMobile ? 'h-[42vh]' : 'h-full',
                transcriptExpanded && !isMobile ? 'w-520px' : isMobile ? 'w-full' : 'w-360px'
              )}
            >
              <MeetingTranscriptPanel
                open={transcriptOpen}
                expanded={transcriptExpanded}
                segments={segments}
                loading={segmentsLoading}
                status={selected.status}
                search={transcriptQuery}
                onSearchChange={handleSearch}
                onClose={() => setTranscriptOpen(false)}
                onToggleExpanded={() => setTranscriptExpanded((value) => !value)}
                editingSegmentId={editingSegmentId}
                editingText={editingText}
                editSaving={editSaving}
                onBeginEdit={beginEditSegment}
                onEditingTextChange={setEditingText}
                onSaveEdit={() => void saveEditSegment()}
                onCancelEdit={() => {
                  setEditingSegmentId(null);
                  setEditingText('');
                }}
              />
            </div>
          ) : null}
        </div>
      ) : null}

      <VoiceprintModal
        visible={voiceprintOpen}
        mode='manage'
        voiceprints={voiceprints}
        onCancel={() => setVoiceprintOpen(false)}
        onEnroll={async (displayName) => {
          await enrollVoiceprint(displayName);
        }}
        onDelete={deleteVoiceprint}
      />
    </div>
  );
};

export default MeetingDetailPage;
