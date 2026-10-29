import React from 'react';
import { useTranslation } from 'react-i18next';
import VoiceErrorStatus from './VoiceErrorStatus';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

export default function VoiceConversation({ snapshot }: { snapshot: VoiceSnapshot | null }) {
  const { t } = useTranslation();
  const turns = snapshot?.conversation?.length ? snapshot.conversation : snapshot ? [{ turn_id: snapshot.turn_id ?? 'current', user_text: snapshot.user_text, understood_request: snapshot.understood_request, assistant_text: snapshot.assistant_text, model_text: null, reply_state: null, error: null }] : [];
  return <div className='flex flex-col gap-4'>
    {!snapshot?.user_text && !snapshot?.conversation?.some((turn) => turn.user_text || turn.assistant_text) && <div role='status' className='text-12px text-t-secondary'>
      <div>{t('voice.session.userText')}</div>
      <p className='m-0'>{t(snapshot?.phase === 'listening' || snapshot?.phase === 'follow_up' || snapshot?.phase === 'text_follow_up' ? 'voice.session.captureHint' : snapshot?.phase === 'transcribing' ? 'voice.session.transcribingHint' : 'voice.session.emptyHint')}</p>
    </div>}
    {turns.map((turn) => <article key={turn.turn_id} className='flex flex-col gap-2'>
      {turn.user_text && <div className='self-end max-w-85% rounded-lg bg-fill-1 px-3 py-2 text-t-primary'><div className='text-12px text-t-secondary'>{t('voice.session.userText')}</div><p className='whitespace-pre-wrap break-words m-0 text-14px leading-relaxed'>{turn.user_text}</p></div>}
      {turn.understood_request && <details><summary className='text-12px text-t-secondary cursor-pointer'>{t('voice.session.understoodRequest')}</summary><p className='whitespace-pre-wrap break-words m-0'>{turn.understood_request}</p></details>}
      {snapshot?.phase === 'deciding' && turn.turn_id === snapshot.turn_id && !turn.assistant_text && <p role='status' className='m-0 text-12px text-t-secondary'>{t('voice.session.replyPending')}</p>}
      {turn.assistant_text && <div className='text-14px leading-relaxed text-t-primary'><div className='text-12px text-t-secondary'>{t('voice.session.assistantText')}</div><p className='whitespace-pre-wrap break-words m-0'>{turn.assistant_text}</p></div>}
      {turn.reply_state === 'streaming' && <p role='status' className='m-0 text-12px text-t-secondary'>{t('voice.session.replyStreaming')}</p>}
      {turn.reply_state === 'interrupted' && <p role='status' className='m-0 text-12px text-t-secondary'>{t('voice.session.replyIncomplete')}</p>}
      {turn.turn_id !== snapshot?.turn_id && turn.error && <VoiceErrorStatus error={turn.error} />}
    </article>)}
  </div>;
}
