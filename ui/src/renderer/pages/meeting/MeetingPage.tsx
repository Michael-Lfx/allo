import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Button, Empty, Input, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Plus, Search, Voice } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { defaultMeetingTitle, formatRelativeTime, groupSessionsByDate, isLiveSession, notesPreview } from './format';
import { useMeetings } from './useMeetings';
import './meeting.css';

const MeetingPage: React.FC = () => {
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const {
    sessions,
    detectedApp,
    loading,
    lastError,
    setLastError,
    createSession,
    startSession,
    refreshDetectedApps,
  } = useMeetings();

  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [dismissedDetection, setDismissedDetection] = useState<string | null>(null);

  const liveSession = useMemo(
    () => sessions.find((session) => isLiveSession(session)) ?? null,
    [sessions]
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((session) => {
      const hay = `${session.title} ${session.notes?.summary ?? ''}`.toLowerCase();
      return hay.includes(q);
    });
  }, [query, sessions]);

  const groups = useMemo(() => groupSessionsByDate(filtered), [filtered]);
  const showDetection = Boolean(detectedApp) && dismissedDetection !== detectedApp;

  const openSession = useCallback(
    (sessionId: string) => {
      navigate(`/meeting/${sessionId}`);
    },
    [navigate]
  );

  const createAndOpen = useCallback(
    async (title: string, start: boolean) => {
      setCreating(true);
      try {
        const session = await createSession({ title });
        if (start) {
          try {
            await startSession(session.session_id);
          } catch (err) {
            Message.error(String(err));
          }
        }
        navigate(`/meeting/${session.session_id}`);
      } catch (err) {
        Message.error(String(err));
      } finally {
        setCreating(false);
      }
    },
    [createSession, navigate, startSession]
  );

  const handleNew = useCallback(() => {
    void createAndOpen(defaultMeetingTitle(i18n.language), false);
  }, [createAndOpen, i18n.language]);

  const handleTakeNotes = useCallback(() => {
    const title = detectedApp
      ? t('meeting.detected.title', { app: detectedApp })
      : defaultMeetingTitle(i18n.language);
    void createAndOpen(title, true);
  }, [createAndOpen, detectedApp, i18n.language, t]);

  useEffect(() => {
    if (!lastError) return;
    Message.error(t('meeting.error.event', { message: lastError }));
    setLastError(null);
  }, [lastError, setLastError, t]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void refreshDetectedApps();
    }, 8000);
    return () => window.clearInterval(timer);
  }, [refreshDetectedApps]);

  return (
    <div
      className={classNames(
        'meeting-page size-full box-border overflow-y-auto',
        isMobile ? 'px-16px py-14px' : 'px-12px py-24px md:px-40px md:py-32px'
      )}
    >
      <div className={classNames('mx-auto flex w-full max-w-720px flex-col', isMobile ? 'gap-16px' : 'gap-22px')}>
        <div className='flex items-start justify-between gap-12px'>
          <div className='min-w-0'>
            <h1 className={classNames('m-0 font-bold text-t-primary', isMobile ? 'text-24px' : 'text-28px')}>
              {t('meeting.title')}
            </h1>
            <p className='m-0 mt-6px text-13px leading-20px text-t-secondary'>{t('meeting.description')}</p>
          </div>
          <Button type='primary' icon={<Plus theme='outline' size={14} />} loading={creating} onClick={handleNew}>
            {t('meeting.newMeeting')}
          </Button>
        </div>

        {liveSession ? (
          <button
            type='button'
            className='meeting-live-banner appearance-none w-full text-left !border-0 !outline-none flex items-center gap-10px rounded-12px px-14px py-10px'
            onClick={() => openSession(liveSession.session_id)}
          >
            <span className='meeting-live-dot' />
            <span className='min-w-0 flex-1 truncate text-13px font-medium text-t-primary'>
              {liveSession.title}
            </span>
            <span className='text-12px text-t-secondary'>{t('meeting.liveBanner.open')}</span>
          </button>
        ) : null}

        {showDetection ? (
          <div className='flex flex-wrap items-center gap-10px rounded-12px bg-fill-2 px-14px py-10px'>
            <span className='meeting-app-mark'>{(detectedApp ?? '?').slice(0, 1).toUpperCase()}</span>
            <div className='min-w-0 flex-1'>
              <div className='text-13px font-medium text-t-primary'>
                {t('meeting.detected.banner', { app: detectedApp })}
              </div>
              <div className='text-12px text-t-tertiary'>{t('meeting.detected.hint')}</div>
            </div>
            <Button size='small' type='primary' loading={creating} onClick={handleTakeNotes}>
              {t('meeting.detected.cta')}
            </Button>
            <Button size='small' onClick={() => setDismissedDetection(detectedApp)}>
              {t('meeting.detected.dismiss')}
            </Button>
          </div>
        ) : null}

        {sessions.length > 0 ? (
          <Input
            allowClear
            value={query}
            onChange={setQuery}
            prefix={<Search theme='outline' size={14} />}
            placeholder={t('meeting.searchPlaceholder')}
          />
        ) : null}

        {loading ? (
          <div className='flex min-h-240px items-center justify-center'>
            <Spin />
          </div>
        ) : sessions.length === 0 ? (
          <div className='flex min-h-280px flex-col items-center justify-center gap-12px rounded-16px border border-dashed border-[var(--color-border-2)] px-24px py-40px text-center'>
            <Voice theme='outline' size={32} fill='var(--color-text-3)' />
            <div className='text-16px font-medium text-t-primary'>{t('meeting.empty.title')}</div>
            <p className='m-0 max-w-360px text-13px leading-20px text-t-secondary'>{t('meeting.empty.description')}</p>
            <Button type='primary' loading={creating} onClick={handleTakeNotes}>
              {t('meeting.empty.cta')}
            </Button>
          </div>
        ) : filtered.length === 0 ? (
          <Empty description={t('meeting.searchEmpty')} />
        ) : (
          <div className='flex flex-col gap-22px'>
            {groups.map((group) => (
              <section key={group.id} className='flex flex-col gap-6px'>
                <h2 className='m-0 px-4px text-12px font-medium text-t-tertiary'>
                  {t(`meeting.groups.${group.id}`)}
                </h2>
                <ul className='m-0 flex list-none flex-col p-0'>
                  {group.sessions.map((session) => {
                    const preview = notesPreview(session);
                    const live = isLiveSession(session);
                    return (
                      <li key={session.session_id}>
                        <button
                          type='button'
                          className='meeting-session-row appearance-none w-full text-left !border-0 !outline-none !shadow-none flex items-start gap-12px rounded-12px px-12px py-10px'
                          onClick={() => openSession(session.session_id)}
                        >
                          <span className='mt-2px flex size-22px items-center justify-center text-t-tertiary'>
                            {live ? (
                              <span className='meeting-live-dot' />
                            ) : (
                              <Voice theme='outline' size={16} fill='currentColor' />
                            )}
                          </span>
                          <span className='min-w-0 flex-1'>
                            <span className='block truncate text-14px font-medium text-t-primary'>
                              {session.title || t('meeting.untitled')}
                            </span>
                            <span className='mt-4px flex flex-wrap items-center gap-6px text-12px text-t-tertiary'>
                              <span>
                                {formatRelativeTime(session.updated_at_ms || session.created_at_ms, i18n.language)}
                              </span>
                              {preview ? (
                                <>
                                  <span aria-hidden>·</span>
                                  <span className='min-w-0 truncate'>{preview}</span>
                                </>
                              ) : session.status === 'failed' ? (
                                <>
                                  <span aria-hidden>·</span>
                                  <span>{t('meeting.status.failed')}</span>
                                </>
                              ) : null}
                            </span>
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

export default MeetingPage;
