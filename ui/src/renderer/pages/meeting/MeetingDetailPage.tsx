import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { Alert, Button, Dropdown, Empty, Menu, Modal, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { More } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import MeetingAdvancedPanel from './MeetingAdvancedPanel';
import MeetingNotesPane from './MeetingNotesPane';
import MeetingRecordingDock from './MeetingRecordingDock';
import MeetingTranscriptPanel from './MeetingTranscriptPanel';
import VoiceprintModal from './VoiceprintModal';
import { formatNotesMarkdown, formatRelativeTime, isLiveSession } from './format';
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
    voiceprints,
    capabilityDegrade,
    listenStatus,
    loading,
    lastError,
    setLastError,
    startSession,
    stopSession,
    bindConversation,
    updateTitle,
    enrollVoiceprint,
    deleteVoiceprint,
    generateNotes,
    startListen,
    stopListen,
  } = useMeetings({ sessionId });

  const [title, setTitle] = useState('');
  const [actionBusy, setActionBusy] = useState(false);
  const [notesBusy, setNotesBusy] = useState(false);
  const [voiceprintOpen, setVoiceprintOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [transcriptOpen, setTranscriptOpen] = useState(false);
  const [transcriptExpanded, setTranscriptExpanded] = useState(false);
  const titleTimerRef = useRef<number | null>(null);
  const titleFocusedRef = useRef(false);
  const openedForLiveRef = useRef<string | null>(null);

  useEffect(() => {
    if (titleFocusedRef.current) return;
    setTitle(selected?.title ?? '');
  }, [selected?.session_id, selected?.title]);

  useEffect(() => {
    if (!selected) return;
    if (isLiveSession(selected) && openedForLiveRef.current !== selected.session_id) {
      openedForLiveRef.current = selected.session_id;
      setTranscriptOpen(true);
    }
  }, [selected]);

  useEffect(() => {
    if (!lastError) return;
    Message.error(t('meeting.error.event', { message: lastError }));
    setLastError(null);
  }, [lastError, setLastError, t]);

  useEffect(() => {
    return () => {
      if (titleTimerRef.current) window.clearTimeout(titleTimerRef.current);
    };
  }, []);

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

  const handleTitleBlur = useCallback(() => {
    titleFocusedRef.current = false;
    if (!selected) return;
    const trimmed = title.trim();
    if (!trimmed) {
      setTitle(selected.title);
      return;
    }
    if (titleTimerRef.current) window.clearTimeout(titleTimerRef.current);
    if (trimmed !== selected.title) {
      void updateTitle(selected.session_id, trimmed).catch((err) => Message.error(String(err)));
    }
  }, [selected, title, updateTitle]);

  const run = useCallback(async (task: () => Promise<unknown>) => {
    setActionBusy(true);
    try {
      await task();
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, []);

  const handleStart = useCallback(() => {
    if (!selected) return;
    setTranscriptOpen(true);
    void run(() => startSession(selected.session_id));
  }, [run, selected, startSession]);

  const handleStop = useCallback(() => {
    if (!selected) return;
    void run(() => stopSession(selected.session_id));
  }, [run, selected, stopSession]);

  const handleCopyNotes = useCallback(async () => {
    if (!selected) return;
    try {
      await navigator.clipboard.writeText(formatNotesMarkdown(selected));
      Message.success(t('meeting.copySuccess'));
    } catch {
      Message.error(t('meeting.copyFailed'));
    }
  }, [selected, t]);

  const handleGenerateNotes = useCallback(async () => {
    if (!selected) return;
    setNotesBusy(true);
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
      setNotesBusy(false);
    }
  }, [generateNotes, selected, t]);

  const missing = !loading && sessionId && !sessions.some((item) => item.session_id === sessionId) && !selected;
  const canGenerate = selected
    ? (selected.status === 'stopped' || selected.status === 'failed') && selected.notes_status !== 'generating'
    : false;

  return (
    <div className='meeting-detail relative flex size-full min-h-0 w-full flex-1 flex-col overflow-hidden'>
      {loading && !selected ? (
        <div className='flex size-full items-center justify-center'>
          <Spin />
        </div>
      ) : missing ? (
        <div className='flex size-full flex-col items-center justify-center gap-12px'>
          <Empty description={t('meeting.notFound')} />
          <Button onClick={() => navigate('/meeting')}>{t('meeting.back')}</Button>
        </div>
      ) : selected ? (
        <>
          <div className='meeting-detail-scroll min-h-0 flex-1 overflow-y-auto'>
            <div
              className={classNames(
                'mx-auto flex w-full max-w-896px flex-col pb-128px',
                isMobile ? 'px-16px pt-16px' : 'px-24px pt-24px'
              )}
            >
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
                onFocus={() => {
                  titleFocusedRef.current = true;
                }}
                onBlur={handleTitleBlur}
                placeholder={t('meeting.titlePlaceholder')}
              />

              <div className='mb-20px mt-4px flex flex-wrap items-center gap-4px text-13px text-t-tertiary'>
                <span>
                  {t('meeting.edited', {
                    date: formatRelativeTime(selected.updated_at_ms || selected.created_at_ms, i18n.language),
                  })}
                </span>
                <Dropdown
                  trigger='click'
                  position='bl'
                  droplist={
                    <Menu>
                      <Menu.Item key='copy' disabled={!selected.notes} onClick={() => void handleCopyNotes()}>
                        {t('meeting.copyNotes')}
                      </Menu.Item>
                      {canGenerate ? (
                        <Menu.Item key='generate' disabled={notesBusy} onClick={() => void handleGenerateNotes()}>
                          {selected.notes ? t('meeting.notes.regenerate') : t('meeting.notes.generate')}
                        </Menu.Item>
                      ) : null}
                      <Menu.Item key='voiceprint' onClick={() => setVoiceprintOpen(true)}>
                        {t('meeting.voiceprint.manage')}
                      </Menu.Item>
                      <Menu.Item key='advanced' onClick={() => setAdvancedOpen(true)}>
                        {t('meeting.advanced.title')}
                      </Menu.Item>
                    </Menu>
                  }
                >
                  <button type='button' className='meeting-notes-icon-btn' aria-label={t('meeting.advanced.title')}>
                    <More theme='outline' size={16} fill='currentColor' />
                  </button>
                </Dropdown>
              </div>

              <MeetingNotesPane session={selected} generating={notesBusy} />
            </div>
          </div>

          <div className='meeting-cluster pointer-events-none absolute inset-x-0 bottom-16px z-40'>
            <div className='mx-auto flex w-full max-w-896px flex-col items-center gap-8px px-24px'>
              <div
                className={classNames(
                  'meeting-transcript-overlay',
                  transcriptOpen && 'meeting-transcript-overlay--open',
                  transcriptOpen && transcriptExpanded && 'meeting-transcript-overlay--expanded'
                )}
              >
                <MeetingTranscriptPanel
                  expanded={transcriptExpanded}
                  segments={segments}
                  loading={segmentsLoading}
                  status={selected.status}
                  onClose={() => setTranscriptOpen(false)}
                  onToggleExpanded={() => setTranscriptExpanded((value) => !value)}
                />
              </div>
              <MeetingRecordingDock
                session={selected}
                busy={actionBusy}
                transcriptOpen={transcriptOpen}
                onToggleTranscript={() => setTranscriptOpen((open) => !open)}
                onStart={handleStart}
                onStop={handleStop}
              />
            </div>
          </div>
        </>
      ) : null}

      <Modal
        title={t('meeting.advanced.title')}
        visible={advancedOpen}
        footer={null}
        onCancel={() => setAdvancedOpen(false)}
        unmountOnExit
      >
        {selected ? (
          <MeetingAdvancedPanel
            session={selected}
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
            onOpenVoiceprint={() => {
              setAdvancedOpen(false);
              setVoiceprintOpen(true);
            }}
          />
        ) : null}
      </Modal>

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
