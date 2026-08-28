/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('support chat structure', () => {
  test('SiderUserMenu wires support chat between skin and logout with unread badge', () => {
    const source = readSource(
      new URL('../../components/layout/Sider/SiderUserMenu.tsx', import.meta.url)
    );
    expect(source.includes('useSupportChat')).toBe(true);
    expect(source.includes('contactSupport')).toBe(true);
    expect(source.includes('hasUnread')).toBe(true);
    expect(source.includes('openSupportChat')).toBe(true);
    expect(source.includes('min-w-20px h-20px')).toBe(true);
    expect(source.includes('text-11px font-600 leading-20px')).toBe(true);

    const menuSection = source.slice(source.indexOf('const menuContent'));
    const skinIndex = menuSection.indexOf('common.userMenu.changeSkin');
    const supportIndex = menuSection.indexOf('common.userMenu.contactSupport');
    const logoutIndex = menuSection.indexOf('common.userMenu.logout');
    expect(skinIndex).toBeGreaterThan(-1);
    expect(supportIndex).toBeGreaterThan(skinIndex);
    expect(logoutIndex).toBeGreaterThan(supportIndex);
  });

  test('modal and composer keep async chat affordances without attachments or online status', () => {
    const modal = readSource(new URL('./components/SupportChatModal.tsx', import.meta.url));
    const list = readSource(new URL('./components/SupportMessageList.tsx', import.meta.url));
    const composer = readSource(new URL('./components/SupportMessageComposer.tsx', import.meta.url));

    expect(modal.includes('ModalWrapper')).toBe(true);
    expect(modal.includes('common.supportChat.asyncHint')).toBe(false);
    expect(modal.includes('common.supportChat.subtitle')).toBe(false);
    expect(list.includes("aria-live='polite'") || list.includes('aria-live="polite"')).toBe(true);
    expect(composer.includes('Shift+Enter') || composer.includes('shiftKey')).toBe(true);
    expect(composer.includes('isComposing')).toBe(true);
    expect(composer.includes('box-border')).toBe(true);
    expect(composer.includes('Trigger')).toBe(true);
    expect(composer.includes("position='tl'")).toBe(true);
    expect(composer.includes('ImageFiles')).toBe(true);
    expect(composer.includes('FileText')).toBe(true);
    expect(composer.includes('common.supportChat.uploadImage')).toBe(true);
    expect(composer.includes('common.supportChat.uploadLogs')).toBe(true);
    expect(composer.includes('Modal.confirm')).toBe(true);
    expect(composer.includes('common.supportChat.uploadLogsConfirm')).toBe(true);
    expect(composer.includes('collectSupportLogUserInfo')).toBe(true);
    expect(composer.includes('collectSupportDeviceInfo')).toBe(true);
    expect(composer.includes('supportChatApi.packLogs')).toBe(true);
    expect(composer.includes('uploadLogFromPath')).toBe(true);
    expect(composer.includes('useCloudAuth')).toBe(true);
    const uploadFlow = composer.slice(
      composer.indexOf('const prepareAndUploadLogs'),
      composer.indexOf('const showUploadLogsConfirm')
    );
    expect(uploadFlow.includes('await onSend(')).toBe(true);
    expect(uploadFlow.includes('setLogPayload')).toBe(false);
    expect(composer.includes('const [logPayload')).toBe(false);
    expect(composer.includes('{logPayload ?')).toBe(false);
    expect(composer.includes("accept='image/png,image/jpeg,.png,.jpg,.jpeg'")).toBe(true);
    expect(composer.includes('multiple')).toBe(true);
    expect(composer.includes('MAX_IMAGES = 4')).toBe(true);
    expect(composer.includes('MAX_IMAGE_BYTES = 5 * 1024 * 1024')).toBe(true);
    expect(composer.includes('imagePreviews')).toBe(true);
    expect(composer.includes('common.supportChat.removeImage')).toBe(true);
    expect(composer.includes('common.supportChat.imageLimitReached')).toBe(true);
    expect(composer.includes('size-72px')).toBe(true);
    expect(composer.includes('URL.createObjectURL')).toBe(true);
    expect(composer.includes("d='M5 5l10 10M15 5 5 15'")).toBe(true);
    expect(composer.includes('w-148px p-4px')).toBe(true);
    expect(composer.includes('h-32px px-8px')).toBe(true);
    expect(composer.includes("size='16'")).toBe(true);
    expect(composer.includes('support-chat-send-arrow')).toBe(true);
    expect(composer.includes("stroke='currentColor'")).toBe(true);
    expect(composer.includes("strokeWidth='1.5'")).toBe(true);
    expect(composer.includes("M8 3v8.5")).toBe(true);
    expect(composer.includes('min-h-104px')).toBe(true);
    expect(composer.includes('px-6px pt-6px pb-4px')).toBe(true);
    expect(composer.includes('min-h-60px max-h-120px')).toBe(true);
    expect(composer.includes('h-32px flex items-center justify-between')).toBe(true);
    expect(composer.includes('size-28px')).toBe(true);
    expect(composer.match(/data-button-shape='circle'/g)?.length).toBe(2);
    expect(composer.includes('textareaRef')).toBe(true);

    const joined = `${modal}\n${list}\n${composer}`;
    expect(joined.includes('online') || joined.includes('在线')).toBe(false);
    expect(joined.includes('conversation list') || joined.includes('会话列表')).toBe(false);
  });

  test('zh/en locale keys cover support chat copy', () => {
    const zh = JSON.parse(
      readSource(new URL('../../services/i18n/locales/zh-CN/common.json', import.meta.url))
    ) as { supportChat: Record<string, string>; userMenu: Record<string, string> };
    const en = JSON.parse(
      readSource(new URL('../../services/i18n/locales/en-US/common.json', import.meta.url))
    ) as { supportChat: Record<string, string>; userMenu: Record<string, string> };

    const required = [
      'title',
      'addAttachment',
      'uploadImage',
      'uploadLogs',
      'uploadLogsConfirm',
      'uploadLogsReady',
      'uploadLogsFailed',
      'uploadLogsDefaultContent',
      'removeLogs',
      'attachmentUnavailable',
      'removeImage',
      'invalidImage',
      'imageLimitReached',
      'sending',
      'sent',
      'sendFailed',
      'retry',
      'connectionError',
      'syncWarning',
      'authRequired',
      'relogin',
    ];
    for (const key of required) {
      expect(typeof zh.supportChat[key]).toBe('string');
      expect(typeof en.supportChat[key]).toBe('string');
    }
    expect(zh.userMenu.contactSupport).toBe('联系客服');
    expect(en.userMenu.contactSupport).toContain('support');
  });
});
