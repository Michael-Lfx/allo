import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import './meetingCaptions.css';

type MeetingCaptionsPayload = {
  visible: boolean;
  text: string;
  speaker: string;
  is_partial: boolean;
  phase: string;
};

const MeetingCaptionsPage: React.FC = () => {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<MeetingCaptionsPayload | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<MeetingCaptionsPayload>('meeting-captions://update', (event) => {
        if (disposed) return;
        setPayload(event.payload);
      });
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!payload?.visible || !payload.text.trim()) {
    return <main className='meeting-captions meeting-captions--empty' />;
  }

  return (
    <main className='meeting-captions'>
      <section
        className={
          payload.is_partial
            ? 'meeting-captions__card meeting-captions__card--partial'
            : 'meeting-captions__card'
        }
        aria-live='polite'
      >
        {payload.speaker ? (
          <span className='meeting-captions__speaker'>{payload.speaker}</span>
        ) : null}
        <span className='meeting-captions__text'>{payload.text}</span>
        {payload.is_partial ? (
          <span className='meeting-captions__badge'>{t('meeting.partial')}</span>
        ) : null}
      </section>
    </main>
  );
};

export default MeetingCaptionsPage;
