import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, Button, Empty, Input, Select, Spin, Switch, Tag } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import type { MeetingSegment, MeetingSession, SttBackendChoice } from '@/common/adapter/ipcBridge';
import { useMeetings } from './useMeetings';
import VoiceprintModal from './VoiceprintModal';

const STT_OPTIONS: SttBackendChoice[] = ['auto', 'local_sherpa', 'cloud_model_invoke'];

const formatMs = (ms: number): string => {
  const totalSec = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
};

const statusColor = (
  status: MeetingSession['status']
): 'green' | 'orangered' | 'gray' | 'red' | 'arcoblue' | 'gold' => {
  switch (status) {
    case 'recording':
      return 'green';
    case 'paused':
      return 'gold';
    case 'stopping':
      return 'orangered';
    case 'failed':
      return 'red';
    case 'stopped':
      return 'gray';
    case 'created':
      return 'arcoblue';
    default: {
      const _exhaustive: never = status;
      void _exhaustive;
      return 'gray';
    }
  }
};

const channelLabelKey = (channel: MeetingSegment['channel']): string => {
  if (channel === 'mic') return 'meeting.channel.mic';
  if (channel === 'loopback') return 'meeting.channel.loopback';
  return 'meeting.channel.unknown';
};

const MeetingPage: React.FC = () => {
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const { t } = useTranslation();
  const {
    sessions,
    selected,
    selectedId,
    selectSession,
    segments,
    segmentsLoading,
    devices,
    detectedApp,
    voiceprints,
    capabilityDegrade,
    loading,
    lastError,
    setLastError,
    createSession,
    startSession,
    pauseSession,
    resumeSession,
    stopSession,
    bindConversation,
    enrollVoiceprint,
    deleteVoiceprint,
    generateNotes,
    editSegment,
    startListen,
    stopListen,
    listenStatus,
  } = useMeetings();

  const [title, setTitle] = useState('');
  const [sttBackend, setSttBackend] = useState<SttBackendChoice>('auto');
  const [createConversationId, setCreateConversationId] = useState('');
  const [bindId, setBindId] = useState('');
  const [creating, setCreating] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [voiceprintOpen, setVoiceprintOpen] = useState(false);
  const [pendingStartId, setPendingStartId] = useState<string | null>(null);
  const [editingSegmentId, setEditingSegmentId] = useState<string | null>(null);
  const [editingText, setEditingText] = useState('');
  const [editSaving, setEditSaving] = useState(false);

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

  const handleCreate = useCallback(async () => {
    setCreating(true);
    try {
      await createSession({
        title,
        stt_backend: sttBackend,
        bound_conversation_id: createConversationId.trim() || null,
      });
      setTitle('');
      setCreateConversationId('');
      Message.success(t('meeting.createSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setCreating(false);
    }
  }, [createConversationId, createSession, sttBackend, t, title]);

  const runStart = useCallback(
    async (sessionId: string) => {
      setActionBusy(true);
      try {
        await startSession(sessionId);
        Message.success(t('meeting.startSuccess'));
      } finally {
        setActionBusy(false);
        setVoiceprintOpen(false);
        setPendingStartId(null);
      }
    },
    [startSession, t]
  );

  const handleRequestStart = useCallback(() => {
    if (!selected) return;
    setPendingStartId(selected.session_id);
    setVoiceprintOpen(true);
  }, [selected]);

  const handlePause = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      await pauseSession(selected.session_id);
      Message.success(t('meeting.pauseSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [pauseSession, selected, t]);

  const handleResume = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      await resumeSession(selected.session_id);
      Message.success(t('meeting.resumeSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [resumeSession, selected, t]);

  const handleStop = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      await stopSession(selected.session_id);
      Message.success(t('meeting.stopSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [selected, stopSession, t]);

  const handleBind = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      await bindConversation(selected.session_id, bindId.trim() || null);
      Message.success(t('meeting.bindSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [bindConversation, bindId, selected, t]);

  const handleGenerateNotes = useCallback(async () => {
    if (!selected) return;
    setActionBusy(true);
    try {
      const result = await generateNotes(selected.session_id);
      const parts = [t('meeting.notes.generateSuccess')];
      if (result.posted_to_conversation) {
        parts.push(t('meeting.notes.posted'));
      }
      if (result.created_requirement_ids.length > 0) {
        parts.push(
          t('meeting.notes.tasksCreated', { count: result.created_requirement_ids.length })
        );
      }
      Message.success(parts.join(' · '));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setActionBusy(false);
    }
  }, [generateNotes, selected, t]);

  const handleListenToggle = useCallback(
    async (checked: boolean) => {
      if (!selected) return;
      setActionBusy(true);
      try {
        if (checked) {
          const conversationId = bindId.trim() || selected.bound_conversation_id;
          await startListen(selected.session_id, conversationId);
          Message.success(t('meeting.listen.enabled'));
        } else {
          await stopListen(selected.session_id);
          Message.success(t('meeting.listen.disabled'));
        }
      } catch (err) {
        Message.error(String(err));
      } finally {
        setActionBusy(false);
      }
    },
    [bindId, selected, startListen, stopListen, t]
  );

  const beginEditSegment = useCallback((segment: MeetingSegment) => {
    setEditingSegmentId(segment.segment_id);
    setEditingText(segment.text);
  }, []);

  const cancelEditSegment = useCallback(() => {
    setEditingSegmentId(null);
    setEditingText('');
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

  useEffect(() => {
    setBindId(selected?.bound_conversation_id ?? '');
  }, [selected?.bound_conversation_id, selected?.session_id]);

  useEffect(() => {
    if (!lastError) return;
    Message.error(t('meeting.error.event', { message: lastError }));
    setLastError(null);
  }, [lastError, setLastError, t]);

  return (
    <div
      className={classNames(
        'w-full min-h-full box-border overflow-y-auto',
        isMobile ? 'px-16px py-14px' : 'px-12px py-24px md:px-40px md:py-32px'
      )}
    >
      <div
        className={classNames(
          'mx-auto flex w-full max-w-1100px box-border flex-col',
          isMobile ? 'gap-14px' : 'gap-16px'
        )}
      >
        <div className={classNames('flex w-full flex-col', isMobile ? 'gap-6px' : 'gap-8px')}>
          <h1
            className={classNames(
              'm-0 font-bold text-t-primary',
              isMobile ? 'text-24px leading-[1.2]' : 'text-28px leading-[1.15]'
            )}
          >
            {t('meeting.title')}
          </h1>
          <p
            className={classNames(
              'm-0 w-full text-t-secondary',
              isMobile ? 'text-13px leading-20px' : 'text-14px leading-22px'
            )}
          >
            {t('meeting.description')}
          </p>
        </div>

        {detectedApp ? (
          <Alert type='info' content={t('meeting.detectedApps.tip', { app: detectedApp })} />
        ) : (
          <div className='text-12px text-t-tertiary'>{t('meeting.detectedApps.none')}</div>
        )}

        <div className='flex flex-col gap-8px'>
          <div className='text-13px font-medium text-t-primary'>{t('meeting.devices.title')}</div>
          {devices.length === 0 ? (
            <div className='text-13px text-t-secondary'>{t('meeting.devices.empty')}</div>
          ) : (
            <ul className='m-0 flex list-none flex-col gap-4px p-0'>
              {devices.map((device) => (
                <li key={device.id} className='text-13px text-t-secondary'>
                  <span className='text-t-tertiary'>
                    {device.kind === 'input' ? t('meeting.devices.input') : t('meeting.devices.output')}
                  </span>
                  {' · '}
                  <span className='text-t-primary'>{device.name}</span>
                  {device.is_default ? (
                    <span className='ml-6px text-12px text-t-tertiary'>({t('meeting.devices.default')})</span>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className='flex flex-col gap-10px border-b border-b-solid border-b-[var(--color-border-2)] pb-16px'>
          <div className='text-13px font-medium text-t-primary'>{t('meeting.newSession')}</div>
          <div className={classNames('grid gap-10px', isMobile ? 'grid-cols-1' : 'grid-cols-2')}>
            <Input
              value={title}
              onChange={setTitle}
              placeholder={t('meeting.sessionTitlePlaceholder')}
              addBefore={t('meeting.sessionTitle')}
            />
            <div className='flex min-w-0 flex-col gap-4px'>
              <span className='text-12px text-t-tertiary'>{t('meeting.sttBackend')}</span>
              <Select
                value={sttBackend}
                onChange={(value: SttBackendChoice) => setSttBackend(value)}
                options={STT_OPTIONS.map((value) => ({
                  value,
                  label: t(`meeting.stt.${value}`),
                }))}
              />
            </div>
            <Input
              value={createConversationId}
              onChange={setCreateConversationId}
              placeholder={t('meeting.bindConversationPlaceholder')}
              addBefore={t('meeting.bindConversation')}
            />
            <div className='flex items-end'>
              <Button type='primary' loading={creating} onClick={() => void handleCreate()}>
                {t('meeting.create')}
              </Button>
            </div>
          </div>
        </div>

        {loading ? (
          <div className='flex min-h-220px items-center justify-center'>
            <Spin />
          </div>
        ) : (
          <div
            className={classNames(
              'grid w-full gap-16px',
              isMobile ? 'grid-cols-1' : 'grid-cols-[minmax(220px,280px)_minmax(0,1fr)]'
            )}
          >
            <div className='flex min-h-280px flex-col gap-8px'>
              <div className='text-13px font-medium text-t-primary'>{t('meeting.sessions')}</div>
              {sessions.length === 0 ? (
                <Empty description={t('meeting.noSessions')} />
              ) : (
                <ul className='m-0 flex list-none flex-col gap-4px p-0'>
                  {sessions.map((session) => {
                    const active = session.session_id === selectedId;
                    return (
                      <li key={session.session_id}>
                        <button
                          type='button'
                          className={classNames(
                            'appearance-none w-full text-left !border-0 !outline-none !shadow-none rounded-8px px-10px py-8px transition-colors',
                            active
                              ? 'bg-fill-2 text-t-primary'
                              : 'bg-transparent text-t-secondary hover:bg-fill-1 hover:text-t-primary'
                          )}
                          onClick={() => selectSession(session.session_id)}
                        >
                          <div className='flex items-center justify-between gap-8px'>
                            <span className='min-w-0 truncate text-13px font-medium'>{session.title}</span>
                            <Tag size='small' color={statusColor(session.status)}>
                              {t(`meeting.status.${session.status}`)}
                            </Tag>
                          </div>
                          <div className='mt-4px truncate text-12px text-t-tertiary'>
                            {t(`meeting.stt.${session.stt_backend}`)}
                          </div>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>

            <div className='flex min-h-280px flex-col gap-12px'>
              {!selected ? (
                <Empty description={t('meeting.selectSession')} />
              ) : (
                <>
                  {(activeDegrade || sessionCapabilityMessage) && (
                    <Alert
                      type='warning'
                      content={
                        activeDegrade
                          ? t('meeting.capability.banner', { message: activeDegrade.message })
                          : sessionCapabilityMessage
                      }
                    />
                  )}

                  <div className='flex flex-wrap items-center gap-8px'>
                    <Tag color={statusColor(selected.status)}>{t(`meeting.status.${selected.status}`)}</Tag>
                    <span className='text-13px text-t-secondary'>{t(`meeting.stt.${selected.stt_backend}`)}</span>
                  </div>

                  <div className='flex flex-wrap gap-8px'>
                    {selected.status === 'created' ? (
                      <Button type='primary' loading={actionBusy} onClick={handleRequestStart}>
                        {t('meeting.start')}
                      </Button>
                    ) : null}
                    {selected.status === 'recording' ? (
                      <Button loading={actionBusy} onClick={() => void handlePause()}>
                        {t('meeting.pause')}
                      </Button>
                    ) : null}
                    {selected.status === 'paused' ? (
                      <Button type='primary' loading={actionBusy} onClick={() => void handleResume()}>
                        {t('meeting.resume')}
                      </Button>
                    ) : null}
                    {selected.status === 'recording' || selected.status === 'paused' || selected.status === 'stopping' ? (
                      <Button status='danger' loading={actionBusy} onClick={() => void handleStop()}>
                        {t('meeting.stop')}
                      </Button>
                    ) : null}
                    {selected.status === 'stopped' || selected.status === 'failed' ? (
                      <Button
                        type='primary'
                        loading={actionBusy || selected.notes_status === 'generating'}
                        onClick={() => void handleGenerateNotes()}
                      >
                        {t('meeting.notes.generate')}
                      </Button>
                    ) : null}
                  </div>

                  <div className='flex flex-wrap items-center gap-8px'>
                    <Input
                      className='min-w-200px flex-1'
                      value={bindId}
                      onChange={setBindId}
                      placeholder={t('meeting.bindConversationPlaceholder')}
                      addBefore={t('meeting.bindConversation')}
                    />
                    <Button loading={actionBusy} onClick={() => void handleBind()}>
                      {bindId.trim() ? t('meeting.bind') : t('meeting.unbind')}
                    </Button>
                  </div>

                  <div className='flex flex-wrap items-center gap-8px'>
                    <span className='text-13px font-medium text-t-primary'>{t('meeting.listen.title')}</span>
                    <Switch
                      checked={Boolean(listenStatus?.enabled)}
                      loading={actionBusy}
                      onChange={(checked) => void handleListenToggle(checked)}
                    />
                    <span className='text-12px text-t-tertiary'>{t('meeting.listen.hint')}</span>
                    {listenStatus?.enabled ? (
                      <Tag size='small' color='arcoblue'>
                        {t('meeting.listen.windowCount', {
                          count: listenStatus.window_segment_count,
                        })}
                      </Tag>
                    ) : null}
                  </div>

                  <div className='flex flex-col gap-8px'>
                    <div className='flex flex-wrap items-center gap-8px'>
                      <div className='text-13px font-medium text-t-primary'>{t('meeting.notes.title')}</div>
                      <Tag size='small'>{t(`meeting.notes.status.${selected.notes_status}`)}</Tag>
                      {selected.notes ? (
                        <span className='text-12px text-t-tertiary'>
                          {t(`meeting.notes.source.${selected.notes.source}`)}
                        </span>
                      ) : null}
                    </div>
                    {selected.notes_status === 'generating' ? (
                      <div className='text-13px text-t-secondary'>{t('meeting.notes.generating')}</div>
                    ) : selected.notes ? (
                      <div className='flex flex-col gap-10px text-13px text-t-primary'>
                        <div>
                          <div className='mb-4px text-12px text-t-tertiary'>{t('meeting.notes.summary')}</div>
                          <div className='whitespace-pre-wrap leading-20px'>{selected.notes.summary}</div>
                        </div>
                        {selected.notes.decisions.length > 0 ? (
                          <div>
                            <div className='mb-4px text-12px text-t-tertiary'>{t('meeting.notes.decisions')}</div>
                            <ul className='m-0 flex list-disc flex-col gap-2px pl-18px'>
                              {selected.notes.decisions.map((item) => (
                                <li key={item}>{item}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {selected.notes.todos.length > 0 ? (
                          <div>
                            <div className='mb-4px text-12px text-t-tertiary'>{t('meeting.notes.todos')}</div>
                            <ul className='m-0 flex list-disc flex-col gap-2px pl-18px'>
                              {selected.notes.todos.map((item) => (
                                <li key={item.title}>
                                  {item.title}
                                  {item.detail ? (
                                    <span className='text-t-secondary'> — {item.detail}</span>
                                  ) : null}
                                </li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {selected.notes.risks.length > 0 ? (
                          <div>
                            <div className='mb-4px text-12px text-t-tertiary'>{t('meeting.notes.risks')}</div>
                            <ul className='m-0 flex list-disc flex-col gap-2px pl-18px'>
                              {selected.notes.risks.map((item) => (
                                <li key={item}>{item}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {selected.notes.speaker_highlights.length > 0 ? (
                          <div>
                            <div className='mb-4px text-12px text-t-tertiary'>
                              {t('meeting.notes.speakerHighlights')}
                            </div>
                            <ul className='m-0 flex list-disc flex-col gap-2px pl-18px'>
                              {selected.notes.speaker_highlights.map((item) => (
                                <li key={`${item.speaker}-${item.highlight}`}>
                                  <span className='font-medium'>{item.speaker}</span>: {item.highlight}
                                </li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                      </div>
                    ) : (
                      <div className='text-13px text-t-secondary'>{t('meeting.notes.empty')}</div>
                    )}
                  </div>

                  <div className='flex flex-col gap-8px'>
                    <div className='text-13px font-medium text-t-primary'>{t('meeting.timeline')}</div>
                    {segmentsLoading ? (
                      <div className='flex min-h-160px items-center justify-center'>
                        <Spin />
                      </div>
                    ) : segments.length === 0 ? (
                      <Empty description={t('meeting.noSegments')} />
                    ) : (
                      <ul className='m-0 flex max-h-480px list-none flex-col gap-8px overflow-y-auto p-0'>
                        {segments.map((segment) => (
                          <li
                            key={segment.segment_id}
                            className={classNames(
                              'border-b border-b-solid border-b-[var(--color-border-2)] pb-8px',
                              segment.is_partial && 'opacity-70 italic'
                            )}
                          >
                            <div className='mb-4px flex flex-wrap items-center gap-8px text-12px text-t-tertiary'>
                              <span className='font-medium text-t-primary'>
                                {segment.speaker_label || t('meeting.speaker')}
                              </span>
                              <span>
                                {formatMs(segment.start_ms)}–{formatMs(segment.end_ms)}
                              </span>
                              <span>{t(channelLabelKey(segment.channel))}</span>
                              {segment.is_partial ? (
                                <Tag size='small' color='orangered'>
                                  {t('meeting.partial')}
                                </Tag>
                              ) : null}
                              {segment.is_manual_edit ? (
                                <Tag size='small'>{t('meeting.edited')}</Tag>
                              ) : null}
                              {editingSegmentId !== segment.segment_id ? (
                                <Button
                                  size='mini'
                                  type='text'
                                  disabled={editSaving}
                                  onClick={() => beginEditSegment(segment)}
                                >
                                  {t('meeting.edit')}
                                </Button>
                              ) : null}
                            </div>
                            {editingSegmentId === segment.segment_id ? (
                              <div className='flex flex-col gap-8px'>
                                <Input.TextArea
                                  autoSize={{ minRows: 2, maxRows: 8 }}
                                  value={editingText}
                                  onChange={setEditingText}
                                  placeholder={t('meeting.editPlaceholder')}
                                />
                                <div className='flex gap-8px'>
                                  <Button
                                    size='mini'
                                    type='primary'
                                    loading={editSaving}
                                    onClick={() => void saveEditSegment()}
                                  >
                                    {t('meeting.saveEdit')}
                                  </Button>
                                  <Button
                                    size='mini'
                                    disabled={editSaving}
                                    onClick={cancelEditSegment}
                                  >
                                    {t('meeting.cancelEdit')}
                                  </Button>
                                </div>
                              </div>
                            ) : (
                              <div
                                className={classNames(
                                  'text-13px leading-20px text-t-primary whitespace-pre-wrap',
                                  segment.is_partial && 'text-t-secondary'
                                )}
                              >
                                {segment.text || '…'}
                              </div>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </>
              )}
            </div>
          </div>
        )}
      </div>

      <VoiceprintModal
        visible={voiceprintOpen}
        voiceprints={voiceprints}
        onCancel={() => {
          setVoiceprintOpen(false);
          setPendingStartId(null);
        }}
        onSkipAndStart={async () => {
          if (!pendingStartId) return;
          await runStart(pendingStartId);
        }}
        onEnroll={async (displayName) => {
          await enrollVoiceprint(displayName);
        }}
        onEnrollAndStart={async (displayName) => {
          if (!pendingStartId) return;
          await enrollVoiceprint(displayName);
          await runStart(pendingStartId);
        }}
        onDelete={deleteVoiceprint}
      />
    </div>
  );
};

export default MeetingPage;
