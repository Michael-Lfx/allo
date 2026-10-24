import React from 'react';
import {test,expect} from 'bun:test';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import VoiceComposerStatus from './VoiceComposerStatus';
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
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><div data-theme={theme}><VoiceComposerStatus conversationId='p' snapshot={snapshot}/></div></I18nextProvider>);
      expect(html).toContain('P queued B');expect(html).toContain(dictionary.task.cancel);expect(html).not.toContain('Q private result');expect(html).not.toContain('voice.');
    }
    const idle=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceComposerStatus conversationId='p' snapshot={{...snapshot,queue:[]}}/></I18nextProvider>);
    expect(idle).not.toContain('<details');
    expect(idle).not.toContain(dictionary.task.cancel);
    const error=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceComposerStatus conversationId='p' snapshot={null} historyFailed/></I18nextProvider>);
    expect(error).toContain(dictionary.composer.historyFailed);
    expect(error).toContain(dictionary.composer.retryHistory);
  });
}

for(const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const){
 test(`${locale}: each acoustic phase has concise translated status without workspace or diagnostic payload`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
  for(const phase of Object.keys(dictionary.session.phase) as VoiceSnapshot['phase'][]){
   const snapshot={phase,session_id:'s',binding:{session_id:'s',conversation_id:'p',workspace:'D:/private-project'},conversation:[{model_text:'private router JSON'}]} as unknown as VoiceSnapshot;
   const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceComposerStatus conversationId='p' snapshot={snapshot}/></I18nextProvider>);
   expect(html).toContain(dictionary.session.phase[phase]);
   expect(html).not.toContain('D:/private-project');expect(html).not.toContain('private router JSON');
   expect(html).not.toContain('<details');expect(html).not.toContain('voice.session.phase.');
  }
 });
}
