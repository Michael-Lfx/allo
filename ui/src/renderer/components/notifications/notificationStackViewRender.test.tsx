import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import NotificationStackView from './NotificationStackView';
import type { NotificationStackViewProps } from './NotificationStackView';
import type { StoredNotification } from './notificationTypes';

let sequence = 0;
const notice = (overrides: Partial<StoredNotification> = {}): StoredNotification => {
  sequence += 1;
  return {
    key: `key-${sequence}`,
    scopeId: 'scope-test',
    id: `id-${sequence}`,
    level: 'info',
    content: `content-${sequence}`,
    duration: 3000,
    remainingMs: 3000,
    closable: true,
    showIcon: true,
    passthrough: false,
    createdAt: sequence,
    revision: 0,
    status: 'active',
    ...overrides,
  };
};

const labels: NotificationStackViewProps['labels'] = {
  close: '关闭通知',
  collapse: '收起通知',
  more: (count) => `还有 ${count} 条通知`,
  moreLabel: '更多通知',
};

const renderStack = (overrides: Partial<NotificationStackViewProps> = {}): string =>
  renderToStaticMarkup(
    <NotificationStackView
      displayedRecords={[]}
      expanded={false}
      hiddenCount={0}
      shouldScroll={false}
      bottomInset={24}
      livePoliteMessage=''
      liveAssertiveMessage=''
      labels={labels}
      onToggleExpanded={() => {}}
      onDismiss={() => {}}
      onPointerEnter={() => {}}
      onPointerLeave={() => {}}
      onFocusCapture={() => {}}
      onBlurCapture={() => {}}
      onKeyDown={() => {}}
      {...overrides}
    />,
  );

const cardCount = (markup: string): number => markup.split('flowy-notification-card ').length - 1;

describe('NotificationStackView', () => {
  test('collapsed stack shows the given cards and the counter pill with chevron', () => {
    const records = [notice(), notice(), notice()];
    const markup = renderStack({ displayedRecords: records, hiddenCount: 2 });
    expect(cardCount(markup)).toBe(3);
    expect(markup.includes('flowy-notification-stack__counter')).toBe(true);
    expect(markup.includes('flowy-notification-stack__counter-pill')).toBe(true);
    expect(markup.includes('>2</span>')).toBe(true);
    expect(markup.includes('更多通知')).toBe(true);
    expect(markup.includes('aria-label="还有 2 条通知"')).toBe(true);
    expect(markup.includes('aria-expanded="false"')).toBe(true);
    expect(markup.includes('flowy-notification-stack__counter-chevron')).toBe(true);
    expect(markup.includes('>+<') || markup.includes('>−<')).toBe(false);
  });

  test('no counter when nothing is hidden and the stack is collapsed', () => {
    const markup = renderStack({ displayedRecords: [notice()], hiddenCount: 0 });
    expect(markup.includes('flowy-notification-stack__counter')).toBe(false);
  });

  test('expanded stack renders every record, the collapse label, and stagger delays on newly revealed cards', () => {
    const visible = [notice(), notice(), notice()];
    const revealed = [notice(), notice()];
    const markup = renderStack({
      displayedRecords: [...revealed, ...visible],
      expanded: true,
      hiddenCount: 0,
      shouldScroll: true,
      newlyRevealedKeys: new Set(revealed.map((item) => item.key)),
    });
    expect(cardCount(markup)).toBe(5);
    expect(markup.includes('收起通知')).toBe(true);
    expect(markup.includes('aria-expanded="true"')).toBe(true);
    expect(markup.includes('data-expanded="true"')).toBe(true);
    expect(markup.includes('flowy-notification-stack__cards--scrollable')).toBe(true);
    // First revealed card has no delay, the second is delayed by one step.
    expect(markup.includes('animation-delay:24ms')).toBe(true);
    expect(markup.includes('flowy-notification-stack__counter-pill')).toBe(false);
  });

  test('exiting records render with the exiting class and no close button', () => {
    const exiting = notice({ status: 'exiting' });
    const markup = renderStack({ displayedRecords: [exiting] });
    expect(markup.includes('flowy-notification-card--exiting')).toBe(true);
    expect(markup.includes('data-notification-status="exiting"')).toBe(true);
    expect(markup.includes('flowy-notification__close')).toBe(false);
  });

  test('passthrough and persistent markers reach the card class list', () => {
    const markup = renderStack({
      displayedRecords: [notice({ passthrough: true, duration: 0, remainingMs: 0 })],
    });
    expect(markup.includes('flowy-notification-card--passthrough')).toBe(true);
    expect(markup.includes('flowy-notification-card--persistent')).toBe(true);
  });

  test('bottom inset lands on the host style variable', () => {
    const markup = renderStack({ bottomInset: 88 });
    expect(markup.includes('--flowy-notification-bottom-inset:88px')).toBe(true);
  });

  test('two static live regions carry their own routed messages', () => {
    const markup = renderStack({ livePoliteMessage: '已保存', liveAssertiveMessage: '保存失败' });
    expect(markup.includes('role="status"')).toBe(true);
    expect(markup.includes('aria-live="polite"')).toBe(true);
    expect(markup.includes('role="alert"')).toBe(true);
    expect(markup.includes('aria-live="assertive"')).toBe(true);
    expect(markup.includes('已保存')).toBe(true);
    expect(markup.includes('保存失败')).toBe(true);
  });

  test('normal level with showIcon false renders no icon span; semantic levels carry their class', () => {
    const plain = notice({ level: 'normal', showIcon: false });
    const markup = renderStack({ displayedRecords: [plain] });
    expect(markup.includes('flowy-notification__icon')).toBe(false);

    const success = notice({ level: 'success' });
    const second = renderStack({ displayedRecords: [success] });
    expect(second.includes('flowy-notification-card--success')).toBe(true);
    expect(second.includes('flowy-notification__icon')).toBe(true);
  });

  test('close button renders for closable active cards with the localized label', () => {
    const markup = renderStack({ displayedRecords: [notice()] });
    expect(markup.includes('flowy-notification__close')).toBe(true);
    expect(markup.includes('aria-label="关闭通知"')).toBe(true);
  });
});
