import React from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Tag } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Copy } from '@icon-park/react';
import type { MeetingSession } from '@/common/adapter/ipcBridge';
import { formatNotesMarkdown, isLiveSession } from './format';

type MeetingNotesPaneProps = {
  session: MeetingSession;
  generating: boolean;
  onGenerate: () => void;
};

const MeetingNotesPane: React.FC<MeetingNotesPaneProps> = ({ session, generating, onGenerate }) => {
  const { t } = useTranslation();
  const notes = session.notes;
  const live = isLiveSession(session);
  const canGenerate = session.status === 'stopped' || session.status === 'failed';

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(formatNotesMarkdown(session));
      Message.success(t('meeting.copySuccess'));
    } catch {
      Message.error(t('meeting.copyFailed'));
    }
  };

  return (
    <div className='flex flex-col gap-16px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <div className='text-16px font-semibold text-t-primary'>{t('meeting.notes.title')}</div>
        <Tag size='small'>{t(`meeting.notes.status.${session.notes_status}`)}</Tag>
        {notes ? (
          <span className='text-12px text-t-tertiary'>{t(`meeting.notes.source.${notes.source}`)}</span>
        ) : null}
        <div className='ml-auto flex flex-wrap gap-8px'>
          {notes ? (
            <Button size='small' icon={<Copy theme='outline' size={14} />} onClick={() => void handleCopy()}>
              {t('meeting.copyNotes')}
            </Button>
          ) : null}
          {canGenerate ? (
            <Button type='primary' size='small' loading={generating} onClick={onGenerate}>
              {t('meeting.notes.generate')}
            </Button>
          ) : null}
        </div>
      </div>

      {session.notes_status === 'generating' ? (
        <p className='m-0 text-14px leading-22px text-t-secondary'>{t('meeting.notes.generating')}</p>
      ) : notes ? (
        <div className='flex flex-col gap-20px text-14px leading-24px text-t-primary'>
          {notes.summary.trim() ? (
            <p className='m-0 whitespace-pre-wrap'>{notes.summary}</p>
          ) : null}
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
              <h3 className='m-0 mb-8px text-13px font-semibold text-t-secondary'>
                {t('meeting.notes.speakerHighlights')}
              </h3>
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
      ) : (
        <p className='m-0 max-w-520px text-14px leading-22px text-t-secondary'>
          {live ? t('meeting.notes.emptyLive') : t('meeting.notes.emptyIdle')}
        </p>
      )}
    </div>
  );
};

export default MeetingNotesPane;
