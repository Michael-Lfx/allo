import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Button, Spin } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { FileText, Plus } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { defaultMeetingTitle, formatRelativeTime, groupSessionsByDate, isLiveSession } from './format';
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

  const [creating, setCreating] = useState(false);
  const [dismissedDetection, setDismissedDetection] = useState<string | null>(null);

  const groups = useMemo(() => groupSessionsByDate(sessions), [sessions]);
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
        isMobile ? 'px-16px py-16px' : 'px-24px py-32px md:px-40px'
      )}
    >
      <div className='mx-auto flex w-full max-w-896px flex-col gap-24px'>
        <div className='flex items-center justify-between gap-12px'>
          <h1 className='m-0 text-20px font-bold text-t-primary'>{t('meeting.title')}</h1>
          <Button size='small' icon={<Plus theme='outline' size={14} />} loading={creating} onClick={handleNew}>
            {t('meeting.newMeeting')}
          </Button>
        </div>

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

        {loading ? (
          <div className='flex min-h-240px items-center justify-center'>
            <Spin />
          </div>
        ) : sessions.length === 0 ? (
          <div className='space-y-8px rounded-12px border border-dashed border-[var(--color-border-2)] px-24px py-28px text-center'>
            <FileText theme='outline' size={28} fill='var(--color-text-3)' />
            <p className='m-0 mt-8px text-13px text-t-secondary'>{t('meeting.empty.title')}</p>
            <p className='m-0 text-12px text-t-tertiary'>{t('meeting.empty.description')}</p>
          </div>
        ) : (
          <div className='flex flex-col gap-24px'>
            {groups.map((group) => (
              <section key={group.id} className='flex flex-col gap-4px'>
                <h2 className='m-0 px-12px text-13px font-medium text-t-tertiary'>{t(`meeting.groups.${group.id}`)}</h2>
                <ul className='m-0 flex list-none flex-col p-0'>
                  {group.sessions.map((session) => {
                    const live = isLiveSession(session);
                    return (
                      <li key={session.session_id}>
                        <button
                          type='button'
                          className='meeting-session-row appearance-none w-full text-left !border-0 !outline-none !shadow-none flex items-start gap-12px rounded-8px px-12px py-8px'
                          onClick={() => openSession(session.session_id)}
                        >
                          <span className='mt-2px flex size-20px items-center justify-center text-t-tertiary'>
                            {live ? (
                              <span className='meeting-live-dot' />
                            ) : (
                              <FileText theme='outline' size={18} fill='currentColor' />
                            )}
                          </span>
                          <span className='min-w-0 flex-1'>
                            <span className='block truncate text-13px font-medium leading-18px text-t-primary'>
                              {session.title || t('meeting.untitled')}
                            </span>
                            <span className='mt-4px block text-12px text-t-tertiary'>
                              {formatRelativeTime(session.updated_at_ms || session.created_at_ms, i18n.language)}
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
