import React from 'react';
import {test,expect} from 'bun:test';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import {VoiceConversationStatus} from './VoiceConversationBar';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
for(const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const){
  test(`${locale}: P queue stays visible while Q runs, after hangup, in both themes`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    const snapshot={instance_id:'host',seq:1,session_id:null,phase:'idle',ended_reason:'user_stop',error:null,
      task:{command_id:'a',status:'accepted',conversation_id:'q',result_text:'Q private result'},
      queue:[{position:1,display_text:'P queued B',workspace:'D:/p',task:{command_id:'b',status:'queued',conversation_id:'p',message_id:'msg-b'}}]} as unknown as VoiceSnapshot;
    for(const theme of ['light','dark']){
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><div data-theme={theme}><VoiceConversationStatus conversationId='p' snapshot={snapshot}/></div></I18nextProvider>);
      expect(html).toContain('P queued B');expect(html).toContain(dictionary.task.cancel);expect(html).not.toContain('Q private result');expect(html).not.toContain('voice.');
    }
    expect(renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversationStatus conversationId='p' snapshot={{...snapshot,queue:[]}}/></I18nextProvider>)).toBe('');
  });
}
