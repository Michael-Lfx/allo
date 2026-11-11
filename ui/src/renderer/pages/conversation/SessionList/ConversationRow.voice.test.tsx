import {expect,test} from 'bun:test';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import {LayoutContext} from '@/renderer/hooks/context/LayoutContext';
import ConversationRow from './ConversationRow';
import type {ConversationRowProps} from './types';
import zh from '@/renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@/renderer/services/i18n/locales/en-US/voice.json';
import {isVoiceConversation} from '../utils/isVoiceConversation';

test('strict voice markers survive rename/serialization; names, paths and model IDs are not type markers',()=>{
 for(const extra of [{voice:true},{is_default_voice:true}]){
  expect(isVoiceConversation(JSON.parse(JSON.stringify({type:'nomi',name:'renamed',extra})))).toBe(true);
 }
 for(const extra of [{voice:'true'},{voice:1},{voice:false},{is_default_voice:'true'},{workspace:'voice',model:'voice'},{}])
  expect(isVoiceConversation({type:'nomi',extra})).toBe(false);
 expect(isVoiceConversation(undefined)).toBe(false);
 expect(isVoiceConversation({type:'acp',extra:{voice:true}})).toBe(false);
});
for(const [locale,voice] of [['zh-CN',zh],['en-US',en]] as const){
 test(`${locale}: rendered list marker is persistent across row states and ordinary tasks have none`,async()=>{
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice}}},initImmediate:false});
  for(const mobile of [false,true])for(const collapsed of [false,true])for(const batch of [false,true])for(const menu of [false,true])for(const generating of [false,true]){
   const props={conversation:{id:'row',type:'nomi',name:'renamed',extra:{voice:true},created_at:Date.now()},
    isGenerating:generating,hasCompletionUnread:true,collapsed,tooltipEnabled:false,batchMode:batch,checked:false,selected:true,menuVisible:menu,
    onToggleChecked:()=>{},onConversationClick:()=>{},onOpenMenu:()=>{},onMenuVisibleChange:()=>{},onEditStart:()=>{},onDelete:()=>{},onTogglePin:()=>{},getJobStatus:()=> 'none'} as unknown as ConversationRowProps;
   const render=(value:ConversationRowProps)=>renderToStaticMarkup(<I18nextProvider i18n={i18n}><LayoutContext.Provider value={{isMobile:mobile,siderCollapsed:false,setSiderCollapsed:()=>{}}}><ConversationRow {...value}/></LayoutContext.Provider></I18nextProvider>);
   const html=render(props);
   expect(html).toContain('data-testid="conversation-voice-marker"');
   expect(html).toContain(`aria-label="${voice.entry.listMarker}"`);
   expect(html).toContain('data-testid="conversation-voice-trailing"');
   expect(html.indexOf('data-testid="conversation-row-title"')).toBeLessThan(html.indexOf('data-testid="conversation-voice-marker"'));
   expect(html).toContain(`data-voice-mobile="${mobile}"`);
   expect(html).toContain(`data-voice-collapsed="${collapsed}"`);
   expect(html).toContain(`data-voice-batch="${batch}"`);
   expect(html).toContain(`data-voice-menu-open="${menu}"`);
   expect(html).not.toContain('animate-pulse');
   expect(html).toContain('text-t-secondary');
   const activeHtml=render({...props,isCallActive:true});
   expect(activeHtml).toContain('text-success');
   expect(activeHtml).toContain('voice-call-wave');
   const normal=render({...props,conversation:{...props.conversation,name:'语音任务',extra:{}} as ConversationRowProps['conversation']});
   expect(normal).not.toContain('conversation-voice-marker');
   expect(normal).not.toContain('conversation-voice-trailing');
  }
 });
}
