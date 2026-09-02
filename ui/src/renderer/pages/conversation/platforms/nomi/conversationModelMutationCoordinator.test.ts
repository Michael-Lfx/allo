/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { createConversationModelMutationCoordinator } from './conversationModelMutationCoordinator';

describe('conversation model mutation coordinator', () => {
  test('skips automatic work queued before an explicit selection', async () => {
    const coordinator = createConversationModelMutationCoordinator();
    const order: string[] = [];
    let releaseBlocker!: () => void;
    let blockerStarted!: () => void;
    const blocker = new Promise<void>((resolve) => {
      releaseBlocker = resolve;
    });
    const started = new Promise<void>((resolve) => {
      blockerStarted = resolve;
    });

    const blocked = coordinator.enqueue(async () => {
      blockerStarted();
      await blocker;
      order.push('blocker');
    });
    await started;
    const observedVersion = coordinator.currentMutationVersion();
    const automatic = coordinator.enqueue(async () => {
      if (!coordinator.canRunAutomaticMutation(observedVersion)) return false;
      order.push('automatic');
      coordinator.markAutomaticMutationApplied();
      return true;
    });
    const ticket = coordinator.beginExplicitSelection();
    const explicit = coordinator.enqueue(async () => {
      if (!coordinator.isLatestExplicitSelection(ticket.requestId)) return false;
      order.push('explicit');
      return true;
    });

    releaseBlocker();
    await blocked;

    expect(await automatic).toBe(false);
    expect(await explicit).toBe(true);
    expect(order).toEqual(['blocker', 'explicit']);
  });

  test('lets an already running automatic update finish before the explicit write', async () => {
    const coordinator = createConversationModelMutationCoordinator();
    const order: string[] = [];
    let releaseAutomatic!: () => void;
    let automaticStarted!: () => void;
    const automaticRelease = new Promise<void>((resolve) => {
      releaseAutomatic = resolve;
    });
    const started = new Promise<void>((resolve) => {
      automaticStarted = resolve;
    });
    const observedVersion = coordinator.currentMutationVersion();

    const automatic = coordinator.enqueue(async () => {
      expect(coordinator.canRunAutomaticMutation(observedVersion)).toBe(true);
      automaticStarted();
      await automaticRelease;
      order.push('automatic');
      coordinator.markAutomaticMutationApplied();
      return true;
    });
    await started;

    const ticket = coordinator.beginExplicitSelection();
    const explicit = coordinator.enqueue(async () => {
      expect(coordinator.isLatestExplicitSelection(ticket.requestId)).toBe(true);
      order.push('explicit');
      return true;
    });

    releaseAutomatic();
    expect(await automatic).toBe(true);
    expect(await explicit).toBe(true);
    expect(order).toEqual(['automatic', 'explicit']);
  });

  test('keeps a newer explicit request authoritative and clears busy state only for it', async () => {
    const coordinator = createConversationModelMutationCoordinator();
    let releaseBlocker!: () => void;
    let blockerStarted!: () => void;
    const started = new Promise<void>((resolve) => {
      blockerStarted = resolve;
    });
    const blocker = coordinator.enqueue(async () => {
      blockerStarted();
      await new Promise<void>((resolve) => {
        releaseBlocker = resolve;
      });
    });
    await started;
    const first = coordinator.beginExplicitSelection();
    const firstOperation = coordinator.enqueue(async () => coordinator.isLatestExplicitSelection(first.requestId));
    const second = coordinator.beginExplicitSelection();
    const secondOperation = coordinator.enqueue(async () => coordinator.isLatestExplicitSelection(second.requestId));

    coordinator.completeExplicitSelection(first.requestId);
    expect(coordinator.isExplicitSelectionInFlight()).toBe(true);
    releaseBlocker();

    await blocker;
    expect(await firstOperation).toBe(false);
    expect(await secondOperation).toBe(true);
    coordinator.completeExplicitSelection(second.requestId);
    expect(coordinator.isExplicitSelectionInFlight()).toBe(false);
  });

  test('runs automatic work scheduled after an explicit selection completes', async () => {
    const coordinator = createConversationModelMutationCoordinator();
    const order: string[] = [];
    const ticket = coordinator.beginExplicitSelection();
    const explicit = coordinator.enqueue(async () => {
      try {
        order.push('explicit');
        return true;
      } finally {
        coordinator.completeExplicitSelection(ticket.requestId);
      }
    });
    const observedVersion = coordinator.currentMutationVersion();
    const automatic = coordinator.enqueue(async () => {
      if (!coordinator.canRunAutomaticMutation(observedVersion)) return false;
      order.push('automatic');
      coordinator.markAutomaticMutationApplied();
      return true;
    });

    expect(await explicit).toBe(true);
    expect(await automatic).toBe(true);
    expect(order).toEqual(['explicit', 'automatic']);
  });
});
