import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import conversationEn from '@renderer/services/i18n/locales/en-US/conversation.json';
import conversationZh from '@renderer/services/i18n/locales/zh-CN/conversation.json';
import guidEn from '@renderer/services/i18n/locales/en-US/guid.json';
import guidZh from '@renderer/services/i18n/locales/zh-CN/guid.json';
import ModelReadinessNotice from './ModelReadinessNotice';

type NoticeResources = { conversation: typeof conversationZh; guid: typeof guidZh };

const locales: ReadonlyArray<readonly [string, NoticeResources]> = [
  ['zh-CN', { conversation: conversationZh, guid: guidZh }],
  ['en-US', { conversation: conversationEn, guid: guidEn }],
];

const renderNotice = async (
  locale: string,
  resources: NoticeResources,
  state: 'missing' | 'unavailable',
  hasDraft: boolean,
): Promise<string> => {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: { [locale]: { translation: resources } },
    initImmediate: false,
  });
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <ModelReadinessNotice state={state} hasDraft={hasDraft} onChooseModel={() => {}} />
    </I18nextProvider>,
  );
};

describe('ModelReadinessNotice rendering', () => {
  for (const [locale, resources] of locales) {
    test(`${locale}: missing model keeps the draft and offers one inline recovery action`, async () => {
      const markup = await renderNotice(locale, resources, 'missing', true);

      expect(markup).toContain('guid-model-readiness-notice');
      expect(markup).toContain('data-state="missing"');
      expect(markup).toContain('role="alert"');
      expect(markup).toContain('aria-live="polite"');
      expect(markup).toContain(resources.conversation.chat.noModelSelected);
      expect(markup).toContain(resources.guid.readiness.draftHeldModel);
      expect(markup).toContain(resources.conversation.chat.openModelSettings);
    });

    test(`${locale}: unavailable model uses its own copy and omits the draft badge without a draft`, async () => {
      const markup = await renderNotice(locale, resources, 'unavailable', false);

      expect(markup).toContain('data-state="unavailable"');
      expect(markup).toContain(resources.conversation.chat.modelUnavailableHint);
      expect(markup).not.toContain(resources.guid.readiness.draftHeldModel);
    });
  }
});
