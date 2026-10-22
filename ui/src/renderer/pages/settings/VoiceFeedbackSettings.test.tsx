import { expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import VoiceFeedbackSettings from './VoiceFeedbackSettings';
import VoiceAudioStatus from '../voiceSession/VoiceAudioStatus';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

for (const [locale, dictionary] of [['zh-CN', zh], ['en-US', en]] as const) {
  test(`${locale}: sound controls remain disabled until real settings arrive`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng:locale, resources:{[locale]:{translation:{voice:dictionary}}}, initImmediate:false });
    const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceFeedbackSettings connected={false} /></I18nextProvider>);
    for (const key of ['title','mode','volume','speakReplies','previewHint','loading','save'] as const) expect(html).toContain(dictionary.feedback[key]);
    expect(html).not.toContain('voice.feedback.');
    expect(html).toContain('disabled');
    expect(html).not.toContain(dictionary.feedback.applied);
  });
  test(`${locale}: cue receipt distinguishes skipped, failed and consumed output`, async () => {
    const i18n = createInstance();
    await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    for (const status of ['skipped','failed','completed'] as const) {
      const snapshot = { feedback:{kind:'recognized',status,reason_code: status==='skipped'?'feedback_muted':null} } as VoiceSnapshot;
      const html = renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceAudioStatus snapshot={snapshot} /></I18nextProvider>);
      expect(html).toContain(dictionary.feedback.cue.recognized);
      expect(html).toContain(dictionary.feedback.receipt[status]);
      expect(html).not.toContain('voice.feedback.');
    }
  });
}
