import {expect,test} from 'bun:test';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import {CloudAuthContext,type CloudAuthContextValue} from '@renderer/hooks/context/CloudAuthContext';
import ComposerSubmitCluster from './ComposerSubmitCluster';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

for (const [locale,voice] of [['zh-CN',zh],['en-US',en]] as const) {
  test(`${locale}: voice primary action is exclusive across task, draft and call states`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
    for(const hasDraft of [false,true])for(const active of [false,true])for(const running of [false,true]){
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
        <ComposerSubmitCluster isVoiceConversation hasDraft={hasDraft} isVoiceActive={active} showStop={running} onStop={()=>{}} onSend={()=>{}} onSpeechTranscript={()=>{}} onStartVoiceCall={()=>{}} onStopVoiceCall={()=>{}}/>
      </CloudAuthContext.Provider></I18nextProvider>);
      expect((html.match(/data-testid="composer-(send|stop|phone)-btn"/g)??[]).length).toBe(1);
      expect(html).toContain(`data-testid="composer-${running?'stop':hasDraft?'send':'phone'}-btn"`);
      if(!running&&!hasDraft)expect(html).toContain(active?voice.session.hangup:voice.session.startCall);
      if(!running&&!hasDraft&&active)expect(html).toContain('arco-btn-status-danger');
      expect(html).not.toContain('voice.session.');
    }
  });
}

for(const [locale,voice] of [['zh-CN',zh],['en-US',en]] as const){
 test(`${locale}: uncertain startup remains recoverable and cannot send or hang up an unbound call`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
  const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
   <ComposerSubmitCluster isVoiceConversation hasDraft isVoiceStartUncertain onSend={()=>{}} onSpeechTranscript={()=>{}} onStartVoiceCall={()=>{}}/>
  </CloudAuthContext.Provider></I18nextProvider>);
  expect(html).toContain(voice.composer.retryStart);expect(html).not.toContain('composer-send-btn');expect(html).not.toContain('sendbox-phone-hangup-button');
 });
 test(`${locale}: active call keeps a small hangup beside the single task/send primary`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
  for(const stop of [false,true]){
   const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><CloudAuthContext.Provider value={{ready:false,status:'unauthenticated'} as CloudAuthContextValue}>
    <ComposerSubmitCluster isVoiceConversation isVoiceActive hasDraft showStop={stop} onStop={()=>{}} onSend={()=>{}} onSpeechTranscript={()=>{}} onStopVoiceCall={()=>{}}/>
   </CloudAuthContext.Provider></I18nextProvider>);
   expect(html).toContain('composer-secondary-hangup');expect(html).toContain('arco-btn-status-danger');
   expect((html.match(/data-testid="composer-(send|stop|phone)-btn"/g)??[]).length).toBe(1);
  }
 });
}
