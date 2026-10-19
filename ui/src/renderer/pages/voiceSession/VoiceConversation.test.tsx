import { expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import VoiceConversation from './VoiceConversation';
import VoiceErrorStatus from './VoiceErrorStatus';
import VoiceAudioStatus from './VoiceAudioStatus';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

for (const [locale, dictionary] of [['zh-CN', zh], ['en-US', en]] as const) {
  test(`${locale}: disconnected audio explains automatic recovery and retained text`, async () => {
    const i18n=createInstance();
    await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceErrorStatus error={{stage:'capture',code:'capture_closed'}} /></I18nextProvider>);
    expect(html).toContain(locale==='zh-CN'?'空闲时自动重试':'retry when idle');
    expect(html).toContain(locale==='zh-CN'?'文字已保留':'Text is preserved');
    expect(html).not.toContain('voice.');
  });
  test(`${locale}: normal silence explains the end without a failure alert`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceErrorStatus error={null} endedReason='initial_silence' /></I18nextProvider>);
    expect(html).toContain(locale === 'zh-CN' ? '没有开始讲话，已结束本次倾听' : 'No speech started. Listening has ended');
    expect(html).toContain('role="status"');
    expect(html).not.toContain('role="alert"');
    expect(html).not.toContain('voice.');
  });
  test(`${locale}: cue playback explains why the listening timer has not started`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const snapshot = { phase: 'listening', listening_deadline_ms: null,
      feedback: { kind: 'wake', status: 'started' } } as unknown as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceAudioStatus snapshot={snapshot} /></I18nextProvider>);
    expect(html).toContain(locale === 'zh-CN' ? '提示音结束后开始计时' : 'timer starts after the cue');
    expect(html).not.toContain(dictionary.session.recording);
    expect(html).not.toContain('voice.');
  });
  test(`${locale}: synthesis is distinct from playback and compact diagnostics leave the reply visible`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const snapshot = { phase: 'synthesizing', user_text: '你好啊。', assistant_text: '你好呀，很高兴听到你的声音。', conversation: [] } as unknown as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><div>{i18n.t('voice.session.phase.synthesizing')}</div><VoiceAudioStatus snapshot={snapshot} compact /><VoiceConversation snapshot={snapshot} /><VoiceErrorStatus error={{stage:'turn',code:'voice_turn_failed'}} /></I18nextProvider>);
    expect(html).toContain(dictionary.session.phase.synthesizing);
    expect(html).not.toContain(dictionary.session.phase.speaking);
    expect(html).toContain(dictionary.audio.turnFailed);
    expect(html).toContain('你好呀，很高兴听到你的声音。');
    expect(html).toContain('<details><summary');
    expect(html).not.toContain('voice.');
  });
  test(`${locale}: listening never leaves the transcript panel blank`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const snapshot = { phase: 'listening', conversation: [], user_text: null } as unknown as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={snapshot} /></I18nextProvider>);
    expect(html).toContain(dictionary.session.userText);
    expect(html).toContain(locale === 'zh-CN' ? '停顿后' : 'pause');
    expect(html).not.toContain('voice.session.');
  });
  test(`${locale}: timeout explains the capture stage and processing explains ASR`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const snapshot = { phase: 'transcribing', conversation: [], user_text: null } as unknown as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={snapshot} /><VoiceErrorStatus error={{stage:'capture',code:'speech_not_detected'}} /></I18nextProvider>);
    expect(html).toContain(dictionary.session.transcribingHint);
    expect(html).toContain(dictionary.audio.speechNotDetected);
    expect(html).not.toContain('voice.');
  });
  test(`${locale}: listening countdown becomes recording after detected speech`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const snapshot = { phase: 'listening', connection: 'connected', listening_deadline_ms: Date.now() + 5000, audio: { last_frame_at_ms: Date.now(), capture_state: 'receiving', rms_milli: 3, peak_milli: 15, vad_ready: true, asr_ready: true, kws_ready: true, utterance_samples: 16000 } } as unknown as VoiceSnapshot;
    const waiting = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceAudioStatus snapshot={snapshot} /></I18nextProvider>);
    expect(waiting).toContain(dictionary.session.startWithin.replace('{{seconds}}', '5'));
    expect(waiting).toContain(dictionary.audio.captured.replace('{{seconds}}', '1.0'));
    expect(waiting).toContain('dBFS');
    const recording = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceAudioStatus snapshot={{...snapshot, listening_deadline_ms: null}} /></I18nextProvider>);
    expect(recording).toContain(dictionary.session.recording);
    expect(recording).not.toContain(dictionary.session.startWithin.replace('{{seconds}}', '5'));
  });
  test(`${locale}: transcript and ignored model decision remain visible after returning to idle`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const output = '{"intent_type":"ignore"}';
    const snapshot = { phase: 'idle', conversation: [{ turn_id: 'ignored-turn', user_text: '实际识别原文', assistant_text: null, model_text: output }] } as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={snapshot} /></I18nextProvider>);
    expect(html).toContain('实际识别原文');
    expect(html).toContain('ignore');
    expect(html).toContain(dictionary.session.userText);
    expect(html).toContain(dictionary.session.modelText);
    expect(html).toContain('<details open="">');
    expect(html).not.toContain(dictionary.session.assistantText);
    expect(html).not.toContain(dictionary.session.understoodRequest);
  });
  test(`${locale}: complete model content and earlier reply stay readable after synthesis fails`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const reply = 'complete answer '.repeat(100) + 'VISIBLE-END';
    const output = 'full model message '.repeat(100) + 'MODEL-END';
    const snapshot = { conversation: [
      { turn_id: 'first', user_text: 'earlier question', understood_request: '用户在询问天气状况。', assistant_text: reply, model_text: output },
      { turn_id: 'second', user_text: 'follow-up', assistant_text: null, model_text: null },
    ] } as VoiceSnapshot;
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={snapshot} /><VoiceErrorStatus error={{ stage: 'playback', code: 'tts_failed' }} /></I18nextProvider>);
    expect(html).toContain('VISIBLE-END');
    expect(html).toContain('MODEL-END');
    expect(html).toContain('earlier question');
    expect(html).toContain(dictionary.session.understoodRequest);
    expect(html).toContain('用户在询问天气状况。');
    expect(html.indexOf('earlier question')).toBeLessThan(html.indexOf('用户在询问天气状况。'));
    expect(html.indexOf('用户在询问天气状况。')).toBeLessThan(html.indexOf('VISIBLE-END'));
    expect(html).toContain(dictionary.session.assistantText);
    expect(html).toContain(dictionary.audio.ttsFailed);
    expect(html).not.toContain('line-clamp');
  });
  test(`${locale}: rate limited speech error shows specific hint and preserves reply text`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const reply = '这是文字回复内容，即使限流也不会丢失。';
    const snapshot = {
      phase: 'idle',
      conversation: [
        { turn_id: 'turn-1', user_text: '用户提问', assistant_text: reply, model_text: '{"action":"chat"}' },
      ],
    } as unknown as VoiceSnapshot;
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <VoiceConversation snapshot={snapshot} />
        <VoiceErrorStatus error={{ stage: 'playback', code: 'tts_rate_limited' }} />
      </I18nextProvider>
    );
    expect(html).toContain(dictionary.audio.ttsRateLimited);
    expect(html).toContain(reply);
    expect(html).toContain('role="alert"');
    expect(html).not.toContain('voice.');
  });
}

for (const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const) {
test(`${locale}: partial answer and earlier timeout remain visible after a new wake`,async()=>{
const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
const snapshot={phase:'listening',turn_id:null,error:null,conversation:[{turn_id:'partial',user_text:'后续问题',assistant_text:'第一句。',reply_state:'interrupted',error:{stage:'dialogue',code:'model_timeout'}}]} as unknown as VoiceSnapshot;
const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={snapshot}/></I18nextProvider>);
expect(html).toContain(locale==='zh-CN'?'回答未完成':'Answer incomplete');expect(html).toContain(dictionary.audio.modelTimeout);expect(html).toContain('第一句。');expect(html).not.toContain('voice.');
const streaming=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceConversation snapshot={{...snapshot,turn_id:'partial',conversation:[{...snapshot.conversation![0],reply_state:'streaming',error:null}]}}/></I18nextProvider>);
expect(streaming).toContain(locale==='zh-CN'?'仍在生成':'Still generating');
});}
