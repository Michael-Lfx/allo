import React from 'react';
import { useTranslation } from 'react-i18next';
import type { MeetingSession } from '@/common/adapter/ipcBridge';
import { isLiveSession } from './format';

type MeetingNotesPaneProps = {
  session: MeetingSession;
  generating: boolean;
};

const MeetingNotesPane: React.FC<MeetingNotesPaneProps> = ({ session, generating }) => {
  const { t } = useTranslation();
  const notes = session.notes;

  if (session.notes_status === 'generating' || (generating && !notes)) {
    return <p className='m-0 text-15px leading-24px text-t-secondary'>{t('meeting.notes.generating')}</p>;
  }

  if (!notes) {
    if (isLiveSession(session)) {
      return <p className='m-0 text-15px leading-24px text-t-tertiary'>{t('meeting.notes.emptyLive')}</p>;
    }
    return null;
  }

  return (
    <div className='flex flex-col gap-22px text-15px leading-26px text-t-primary'>
      {notes.summary.trim() ? <p className='m-0 whitespace-pre-wrap'>{notes.summary}</p> : null}
      {notes.decisions.length > 0 ? (
        <section>
          <h3 className='m-0 mb-8px text-13px font-semibold text-t-secondary'>{t('meeting.notes.decisions')}</h3>
          <ul className='m-0 flex list-disc flex-col gap-4px pl-18px'>
            {notes.decisions.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </section>
      ) : null}
      {notes.todos.length > 0 ? (
        <section>
          <h3 className='m-0 mb-8px text-13px font-semibold text-t-secondary'>{t('meeting.notes.todos')}</h3>
          <ul className='m-0 flex list-disc flex-col gap-4px pl-18px'>
            {notes.todos.map((item) => (
              <li key={`${item.title}-${item.detail}`}>
                {item.title}
                {item.detail ? <span className='text-t-secondary'> — {item.detail}</span> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {notes.risks.length > 0 ? (
        <section>
          <h3 className='m-0 mb-8px text-13px font-semibold text-t-secondary'>{t('meeting.notes.risks')}</h3>
          <ul className='m-0 flex list-disc flex-col gap-4px pl-18px'>
            {notes.risks.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </section>
      ) : null}
      {notes.speaker_highlights.length > 0 ? (
        <section>
          <h3 className='m-0 mb-8px text-13px font-semibold text-t-secondary'>{t('meeting.notes.speakerHighlights')}</h3>
          <ul className='m-0 flex list-disc flex-col gap-4px pl-18px'>
            {notes.speaker_highlights.map((item) => (
              <li key={`${item.speaker}-${item.highlight}`}>
                <span className='font-medium'>{item.speaker}</span>: {item.highlight}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
};

export default MeetingNotesPane;
