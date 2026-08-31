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
    expect(modalSource).toContain('conversation-error-report-description');
    expect(modalSource).toContain('maxLength={MAX_REPORT_DESCRIPTION_CHARS}');
    expect(modalSource).toContain('event.ctrlKey || event.metaKey');
    expect(modalSource).toContain('SupportImagePreviewGrid');
    expect(modalSource).toContain('SUPPORT_IMAGE_ACCEPT');
    expect(modalSource).toContain("submitStatus === 'preparation-failed'");
    expect(modalSource).toContain("submitStatus === 'partial-failure'");
    expect(modalSource).toContain("submitStatus === 'partial-failure') return;");
    expect(modalSource).toContain('role=\'button\'');
    expect(modalSource).toContain('providerOwnedPreviewUrlsRef');
    expect(modalSource).not.toContain('Modal.confirm');
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
      '.support-chat-modal .arco-modal-content',
      '.nomifun-modal-body-content',
      '.conversation-error-report__scroll',
      '.support-message-list {',
      '.support-chat-composer__icon',
      '.nomifun-modal-structured-header > button > .i-icon',
      'line-height: 0;',
      'flex: 0 0 32px;',
    ]) {
      expect(cssSource).toContain(fragment);
    }
    expect(cssSource).toContain('100dvh');
    expect(cssSource).not.toContain('.support-chat-modal .arco-modal-body');
  });

  test('keeps support visibility independent from reducer hydration state', () => {
    expect(providerSource).toContain('modalOpen,');
    expect(supportModalSource).toContain('visible={visible}');
    expect(supportModalSource).toContain('visible={modalOpen}');
  });
});
