/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const messageListSource = readFileSync(new URL('./MessageList.tsx', import.meta.url), 'utf8');
const messageStyles = readFileSync(new URL('./messages.css', import.meta.url), 'utf8');

describe('message list scroll button', () => {
  test('declares its circular shape so the global button contract cannot square it', () => {
    expect(messageListSource.includes("className='message-list-scroll-button'")).toBe(true);
    expect(messageListSource.includes("data-button-shape='circle'")).toBe(true);
    expect(messageStyles.includes('.message-list-scroll-button {')).toBe(true);
    expect(messageStyles.includes('border-radius: 999px;')).toBe(true);
  });
});

describe('message list end spacer', () => {
  test('keeps a modest trailing room under the latest reply', () => {
    expect(messageListSource.includes("className='message-list-end-spacer'")).toBe(true);
    expect(messageListSource.includes("data-streaming=")).toBe(false);
    expect(messageStyles.includes('.message-list-end-spacer {')).toBe(true);
    expect(messageStyles.includes('height: 64px;')).toBe(true);
    expect(messageStyles.includes('height: 30vh;')).toBe(false);
  });
});

describe('message list streaming follow', () => {
  test('pins Virtuoso follow to the same bottom threshold as auto-scroll', () => {
    expect(messageListSource.includes('followOutput={resolveFollowOutput}')).toBe(true);
    expect(messageListSource.includes('atBottomThreshold={FOLLOW_BOTTOM_THRESHOLD_PX}')).toBe(true);
    expect(messageListSource.includes('virtuosoRef,')).toBe(true);
    expect(messageListSource.includes('virtuosoMode: scrollParent != null')).toBe(true);
  });

  test('measures streaming row growth in the same frame so the last line does not bounce', () => {
    expect(messageListSource.includes('skipAnimationFrameInResizeObserver')).toBe(true);
  });

  test('anchors follow to the end spacer so a wrap does not shove then restore the last line', () => {
    expect(messageListSource.includes("style={{ overflowAnchor: 'none' }}")).toBe(false);
    expect(messageListSource.includes('layoutPinKey: list')).toBe(true);
    expect(messageStyles.includes('.message-list-end-spacer')).toBe(true);
    const spacerRule = messageStyles.slice(
      messageStyles.indexOf('.message-list-end-spacer {'),
      messageStyles.indexOf('}', messageStyles.indexOf('.message-list-end-spacer {')) + 1
    );
    expect(spacerRule.includes('overflow-anchor: auto')).toBe(true);
    expect(messageStyles.includes("[data-testid='virtuoso-item-list']")).toBe(true);
    const listRule = messageStyles.slice(
      messageStyles.indexOf("[data-testid='virtuoso-item-list']"),
      messageStyles.indexOf('}', messageStyles.indexOf("[data-testid='virtuoso-item-list']")) + 1
    );
    expect(listRule.includes('overflow-anchor: none')).toBe(true);
  });
});
