import { expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { VoiceDialogueStatus } from './VoiceDialogueStatus';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

for (const [locale, dictionary] of [['zh-CN', zh], ['en-US', en]] as const) {
  test(`${locale}: quick model is read-only and distinguishes loading, unavailable and errors`, async () => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: { voice: dictionary } } }, initImmediate: false });
    const render = (settings: React.ComponentProps<typeof VoiceDialogueStatus>['settings'], failed = false) =>
      renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceDialogueStatus settings={settings} failed={failed} /></I18nextProvider>);
    expect(render(null)).toContain(dictionary.feedback.loading);
    expect(render(null, true)).toContain(dictionary.dialogue.failed);
    expect(render({ dialogue_model: null, effective_model: null, available: false })).toContain(dictionary.dialogue.unavailable);
    const html = render({ dialogue_model: null, effective_model: { provider_id: 'provider', model: 'flash' }, available: true });
    expect(html).toContain('provider / flash');
    expect(html).toContain(dictionary.dialogue.description.replaceAll("'", '&#x27;'));
    expect(html).not.toContain('<select');
    expect(html).not.toContain('voice.dialogue.');
  });
}
