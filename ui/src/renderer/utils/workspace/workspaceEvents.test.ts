/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { afterEach, describe, expect, test } from 'bun:test';
import {
  conversationTarget,
  terminalTarget,
  type SessionTarget,
} from '@/common/types/ids';
import { addEventListener } from '@/renderer/utils/emitter';
import { isWorkspacePanelEventForTarget } from '@/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail';
import {
  dispatchWorkspaceToggleEvent,
  subscribeWorkspaceToggleEvent,
} from './workspaceEvents';

const CONVERSATION_ID = '0190f5fe-7c00-7a00-8000-000000000001';
const OTHER_CONVERSATION_ID = '0190f5fe-7c00-7a00-8000-000000000002';

type WindowGlobal = { window?: unknown };

const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');

function installWindow(): void {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: {},
  });
}

function restoreWindow(): void {
  if (originalWindowDescriptor) {
    Object.defineProperty(globalThis, 'window', originalWindowDescriptor);
    return;
  }
  delete (globalThis as WindowGlobal).window;
}

const cleanups: Array<() => void> = [];

function track<T extends () => void>(cleanup: T): T {
  cleanups.push(cleanup);
  return cleanup;
}

afterEach(() => {
  while (cleanups.length > 0) {
    cleanups.pop()?.();
  }
  restoreWindow();
});

describe('workspace toggle event', () => {
  test('delivers the full payload synchronously to conversation and terminal targets', () => {
    installWindow();
    const conversation = conversationTarget(CONVERSATION_ID);
    const terminal = terminalTarget(CONVERSATION_ID);
    const payloads: SessionTarget[] = [];
    let conversationToggles = 0;
    let terminalToggles = 0;

    track(
      subscribeWorkspaceToggleEvent(conversation, () => {
        conversationToggles += 1;
      }),
    );
    track(
      subscribeWorkspaceToggleEvent(terminal, () => {
        terminalToggles += 1;
      }),
    );
    track(
      addEventListener('workspace.toggle', ({ target }) => {
        payloads.push(target);
      }),
    );

    dispatchWorkspaceToggleEvent(conversation);
    expect(conversationToggles).toBe(1);
    expect(terminalToggles).toBe(0);

    dispatchWorkspaceToggleEvent(terminal);
    expect(conversationToggles).toBe(1);
    expect(terminalToggles).toBe(1);
    expect(payloads).toEqual([conversation, terminal]);
  });

  test('compares both kind and id when matching a target', () => {
    installWindow();
    const conversation = conversationTarget(CONVERSATION_ID);
    let toggles = 0;

    track(
      subscribeWorkspaceToggleEvent(conversation, () => {
        toggles += 1;
      }),
    );

    dispatchWorkspaceToggleEvent(terminalTarget(CONVERSATION_ID));
    expect(toggles).toBe(0);

    dispatchWorkspaceToggleEvent(conversationTarget(OTHER_CONVERSATION_ID));
    expect(toggles).toBe(0);

    dispatchWorkspaceToggleEvent(conversation);
    expect(toggles).toBe(1);
  });

  test('removing one subscriber does not affect the others', () => {
    installWindow();
    const conversation = conversationTarget(CONVERSATION_ID);
    let firstToggles = 0;
    let secondToggles = 0;

    const unsubscribeFirst = subscribeWorkspaceToggleEvent(conversation, () => {
      firstToggles += 1;
    });
    const unsubscribeSecond = track(
      subscribeWorkspaceToggleEvent(conversation, () => {
        secondToggles += 1;
      }),
    );

    dispatchWorkspaceToggleEvent(conversation);
    unsubscribeFirst();
    dispatchWorkspaceToggleEvent(conversation);
    expect(firstToggles).toBe(1);
    expect(secondToggles).toBe(2);
    expect(unsubscribeSecond).toBeTypeOf('function');
  });

  test('cancelling an old target subscription stops old target delivery', () => {
    installWindow();
    const oldTarget = conversationTarget(CONVERSATION_ID);
    const newTarget = conversationTarget(OTHER_CONVERSATION_ID);
    let oldToggles = 0;
    let newToggles = 0;

    const unsubscribeOld = subscribeWorkspaceToggleEvent(oldTarget, () => {
      oldToggles += 1;
    });
    unsubscribeOld();
    track(
      subscribeWorkspaceToggleEvent(newTarget, () => {
        newToggles += 1;
      }),
    );

    dispatchWorkspaceToggleEvent(oldTarget);
    expect(oldToggles).toBe(0);
    expect(newToggles).toBe(0);

    dispatchWorkspaceToggleEvent(newTarget);
    expect(oldToggles).toBe(0);
    expect(newToggles).toBe(1);
  });

  test('returns a no-op dispatch when window is unavailable', () => {
    delete (globalThis as WindowGlobal).window;
    let toggles = 0;
    track(
      subscribeWorkspaceToggleEvent(conversationTarget(CONVERSATION_ID), () => {
        toggles += 1;
      }),
    );

    expect(() => dispatchWorkspaceToggleEvent(conversationTarget(CONVERSATION_ID))).not.toThrow();
    expect(toggles).toBe(0);
  });
});

describe('workspace event targets', () => {
  test('uses both kind and id when matching a session', () => {
    const conversation = conversationTarget(CONVERSATION_ID);
    expect(isWorkspacePanelEventForTarget(conversation, conversation)).toBe(true);
    expect(
      isWorkspacePanelEventForTarget(
        terminalTarget(CONVERSATION_ID),
        conversation,
      ),
    ).toBe(false);
    expect(
      isWorkspacePanelEventForTarget(
        conversationTarget(OTHER_CONVERSATION_ID),
        conversation,
      ),
    ).toBe(false);
  });

  test('rejects unscoped events', () => {
    expect(
      isWorkspacePanelEventForTarget(
        undefined,
        conversationTarget(CONVERSATION_ID),
      ),
    ).toBe(false);
  });
});
