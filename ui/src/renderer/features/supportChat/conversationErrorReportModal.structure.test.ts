/**
 * Source-level contract for the two user-facing support surfaces.
 *
 * These checks complement the browser probe: they make the interaction
 * contract reviewable without mounting Arco's portal in a unit-test DOM.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (url: URL) => readFileSync(url, 'utf8');
const modalSource = read(new URL('./components/ConversationErrorReportModal.tsx', import.meta.url));
const supportModalSource = read(new URL('./components/SupportChatModal.tsx', import.meta.url));
const providerSource = read(new URL('./SupportChatProvider.tsx', import.meta.url));
const submissionSource = read(new URL('./conversationErrorReportSubmission.ts', import.meta.url));
const cssSource = read(new URL('../../styles/arco-override.css', import.meta.url));

describe('conversation error report modal contract', () => {
  test('uses a controlled report form with one scroll owner and stable submit states', () => {
    expect(modalSource).toContain("import NomiModal from '@/renderer/components/base/NomiModal';");
    expect(modalSource).toContain('conversation-error-report__scroll');
    expect(modalSource).toContain('conversation-error-report__editor');
    expect(modalSource).toContain('conversation-error-report__diagnostic-section');
    expect(modalSource).toContain('<Popover');
    expect(modalSource).toContain('aria-expanded={infoOpen}');
    expect(modalSource).toContain("width: 'min(720px, calc(100vw - 32px))'");
    expect(modalSource).toContain('alignCenter');
    expect(modalSource).not.toContain('alignCenter={false}');
    expect(modalSource).toContain("wrapClassName='conversation-error-report__wrapper'");
    expect(modalSource).not.toContain("height: 'min(760px, calc(100dvh - 32px))'");
    expect(modalSource).toContain('conversation-error-report-description');
    expect(modalSource).toContain('maxLength={MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS}');
    expect(modalSource).toContain('event.ctrlKey || event.metaKey');
    expect(modalSource).toContain('SupportImagePreviewGrid');
    expect(modalSource).toContain('SUPPORT_IMAGE_ACCEPT');
    expect(modalSource).toContain("submitStatus === 'preparation-failed'");
    expect(modalSource).toContain("submitStatus === 'partial-failure'");
    expect(modalSource).toContain("submitStatus === 'partial-failure') return;");
    expect(modalSource).toContain('conversation-error-report__attachment');
    expect(modalSource).not.toContain('role=\'button\'');
    expect(modalSource).not.toContain('conversation-error-report__intro');
    expect(modalSource).not.toContain('conversation-error-report__upload');
    expect(modalSource).not.toContain('conversation-error-report__auto-info');
    expect(modalSource).toContain('providerOwnedPreviewUrlsRef');
    expect(modalSource).not.toContain('Modal.confirm');
    expect(modalSource).not.toContain('unmountOnExit');
    expect(modalSource).toContain('MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS');
  });

  test('preserves the exact screenshot preview ownership across pending messages', () => {
    expect(modalSource).toContain('fileName: screenshot.file.name');
    expect(modalSource).toContain('previewUrl: screenshot.url');
    expect(modalSource).toContain('clearDraft({ preserveScreenshotPreviews: true })');
    expect(providerSource).toContain('supportImagePreviewCache.set(message.clientMsgId, message.previewUrl)');
    expect(providerSource).toContain('uploadScreenshot');
    expect(providerSource).toContain('logPayload');
    expect(submissionSource).toContain("return { status: 'partial-failure' }");
  });

  test('hydrates hidden support state before reporting and keeps the existing retry contract', () => {
    expect(providerSource).toContain('fetchConversationSnapshot()');
    expect(providerSource).toContain("dispatch({ type: 'ready', conversation: snapshot.conversation");
    expect(providerSource).toContain('submitConversationErrorReportFlow');
    expect(providerSource).toContain('sendWithClientMsgId');
    expect(submissionSource).toContain('markPendingFailed(remaining.clientMsgId)');
    expect(submissionSource).toContain('buildConversationErrorReportMetadata(context)');
  });

  test('invalidates report work and sensitive previews across auth boundaries', () => {
    expect(providerSource).toContain('reportGenerationRef');
    expect(providerSource).toContain('reportContextKeyRef');
    expect(providerSource).toContain('getConversationErrorReportContextKey');
    expect(providerSource).toContain('supportSessionGenerationRef');
    expect(providerSource).toContain('createSupportSessionGuard');
    expect(providerSource).toContain('authAccountIdRef');
    expect(providerSource).toContain('invalidateSupportSession');
    expect(providerSource).toContain('supportImagePreviewCache.clear()');
    expect(providerSource).toContain('key={reportModalInstanceKey}');
    expect(providerSource).toContain('shouldContinue: () => boolean');
    expect(providerSource).toContain('authState.phase === \'offline\'');
    expect(providerSource).toContain('shouldContinue });');
  });

  test('pins the real NomiModal geometry and removes the dead support body selector', () => {
    for (const fragment of [
      '.conversation-error-report-modal .arco-modal-content',
      '.conversation-error-report-modal > div:has(> .arco-modal-content)',
      '.support-chat-modal .arco-modal-content',
      '.nomifun-modal-body-content',
      '.conversation-error-report__scroll',
      '.support-message-list {',
      '.conversation-error-report__editor',
      '.conversation-error-report__info-trigger',
      '.support-chat-composer__icon',
      '.nomifun-modal-structured-header > button > .i-icon',
      'line-height: 0;',
      'flex: 0 0 var(--support-control-size, 32px);',
      'flex: 0 0 auto;',
    ]) {
      expect(cssSource).toContain(fragment);
    }
    expect(cssSource).toContain('100dvh');
    expect(cssSource).toContain('height: auto;');
    expect(cssSource).toContain('height: 100%;');
    expect(cssSource).toContain('flex: 1 1 auto;');
    expect(cssSource).toContain('.support-message-list--empty');
    expect(cssSource).toContain('max-height: none;');
    expect(cssSource).toContain('height: min(640px, calc(100dvh - 24px)) !important;');
    const supportWrapperCss = cssSource.slice(
      cssSource.indexOf('.support-chat-modal__wrapper'),
      cssSource.indexOf('.conversation-error-report__wrapper')
    );
    expect(supportWrapperCss).not.toContain('display: flex !important;');
    expect(supportWrapperCss).not.toContain('align-items: center;');
    expect(supportWrapperCss).not.toContain('justify-content: center;');
    expect(cssSource).not.toContain('.support-chat-modal .arco-modal-body');
    expect(cssSource).not.toContain('.conversation-error-report__upload');
    expect(cssSource).not.toContain('.conversation-error-report__auto-info');
    expect(cssSource).not.toContain('.support-chat-composer textarea:focus-visible {\n  outline: 2px');
    expect(cssSource).toContain('.conversation-error-report-modal {');

    const feedbackGeometryCss = cssSource.slice(
      cssSource.indexOf('.conversation-error-report-modal .arco-modal-content'),
      cssSource.indexOf('.support-chat-modal .arco-modal-content')
    );
    expect(feedbackGeometryCss).toContain('height: auto;');
    expect(feedbackGeometryCss).not.toContain('\n  height: 100%;');

    const supportGeometryCss = cssSource.slice(
      cssSource.indexOf('.support-chat-modal .arco-modal-content'),
      cssSource.indexOf('.conversation-error-report__header')
    );
    expect(supportGeometryCss).toContain('height: 100%;');
  });

  test('keeps modal centering and one scroll owner across both support surfaces', () => {
    for (const selector of ['.support-chat-modal__wrapper', '.conversation-error-report__wrapper']) {
      const start = cssSource.indexOf(`${selector} {`);
      expect(start).toBeGreaterThan(-1);
      const end = cssSource.indexOf('}', start);
      expect(end).toBeGreaterThan(start);
      const wrapperRule = cssSource.slice(start, end);

      expect(wrapperRule).toContain('overflow: hidden;');
      expect(wrapperRule).not.toContain('overflow: auto;');
    }

    const centeredReportSelector =
      '.arco-modal-wrapper.arco-modal-wrapper-align-center .conversation-error-report-modal';
    const centeredReportRule = cssSource.slice(
      cssSource.indexOf(centeredReportSelector),
      cssSource.indexOf(centeredReportSelector) + 260
    );

    expect(centeredReportRule).toContain('display: inline-flex;');
    expect(cssSource.indexOf(centeredReportSelector)).toBeGreaterThan(
      cssSource.indexOf('.conversation-error-report-modal {')
    );
  });

  test('keeps support visibility independent from reducer hydration state', () => {
    expect(providerSource).toContain('modalOpen,');
    expect(supportModalSource).toContain('visible={visible}');
    expect(supportModalSource).toContain('visible={modalOpen}');
  });

  test('lets Arco own wrapper teardown for every persistent support surface', () => {
    const extractRule = (selector: string) => {
      const start = cssSource.indexOf(`${selector} {`);
      expect(start).toBeGreaterThan(-1);
      const end = cssSource.indexOf('}', start);
      expect(end).toBeGreaterThan(start);
      return cssSource.slice(start, end);
    };

    for (const selector of ['.support-chat-modal__wrapper', '.conversation-error-report__wrapper']) {
      expect(extractRule(selector)).not.toMatch(/display\s*:\s*[^;]+!important/);
    }

    expect(supportModalSource).not.toContain('alignCenter={false}');
    expect(modalSource).not.toContain('alignCenter={false}');
    expect(supportModalSource).toContain('maskClosable={false}');
    expect(modalSource).toContain('maskClosable={false}');
  });
});
