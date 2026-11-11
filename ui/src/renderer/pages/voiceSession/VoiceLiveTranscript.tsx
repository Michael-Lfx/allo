import React, {useLayoutEffect, useRef, useState} from 'react';
import {useTranslation} from 'react-i18next';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import VoiceConversation from './VoiceConversation';

export default function VoiceLiveTranscript({snapshot}:{snapshot:VoiceSnapshot}) {
  const {t}=useTranslation();
  const container=useRef<HTMLDivElement>(null);
  const following=useRef(true);
  const [unread,setUnread]=useState(false);
  const signature=JSON.stringify(snapshot.conversation?.map(turn=>[turn.turn_id,turn.user_text,turn.assistant_text,turn.reply_state]));
  useLayoutEffect(()=>{
    if(following.current&&container.current)container.current.scrollTop=container.current.scrollHeight;
    else setUnread(true);
  },[signature]);
  return <div><div ref={container} className='max-h-52vh overflow-y-auto min-h-0 py-2' onScroll={()=>{
    const element=container.current;
    if(element){following.current=element.scrollHeight-element.clientHeight-element.scrollTop<28;if(following.current)setUnread(false);}
  }}><VoiceConversation snapshot={snapshot}/></div>
    {unread&&<button type='button' className='text-12px text-t-secondary bg-fill-1 rounded px-2 py-1 cursor-pointer border-none' onClick={()=>{
      following.current=true;setUnread(false);if(container.current)container.current.scrollTop=container.current.scrollHeight;
    }}>{t('voice.session.newContent')}</button>}
  </div>;
}
