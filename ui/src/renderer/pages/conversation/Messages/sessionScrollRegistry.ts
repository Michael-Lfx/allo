/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { emitter } from '@/renderer/utils/emitter';

export interface SessionScrollSnapshot {
  /** The exact vertical scroll offset in pixels when the user was viewing this session. */
  scrollTop: number;
  /** True if the user intentionally scrolled away from the bottom to read history. */
  userScrolled: boolean;
  /** Timestamp when the snapshot was recorded. */
  updatedAt: number;
  /** The message ID of the bottom-most message when the user scrolled away. */
  lastReadMessageId?: string;
  /** Unread assistant messages count below viewport. */
  unreadCount?: number;
  /** The message ID of the anchor question/turn currently in the viewing viewport. */
  targetMessageId?: string;
  /** The 0-based turn index corresponding to the left dot indicator. */
  turnIndex?: number;
}

const MAX_SESSION_SCROLL_SNAPSHOTS = 200;

class SessionScrollRegistry {
  private readonly snapshots = new Map<string, SessionScrollSnapshot>();

  /**
   * Save a scroll snapshot for a specific conversation session.
   * Maintains LRU eviction to prevent unbounded memory growth.
   */
  public save(
    conversationId: string | undefined | null,
    snapshot: Omit<SessionScrollSnapshot, 'updatedAt'> & { updatedAt?: number }
  ): void {
    if (!conversationId) return;

    // Refresh LRU order by deleting and re-inserting
    if (this.snapshots.has(conversationId)) {
      this.snapshots.delete(conversationId);
    } else if (this.snapshots.size >= MAX_SESSION_SCROLL_SNAPSHOTS) {
      const oldestKey = this.snapshots.keys().next().value;
      if (oldestKey !== undefined) {
        this.snapshots.delete(oldestKey);
      }
    }

    this.snapshots.set(conversationId, {
      scrollTop: Math.max(0, snapshot.scrollTop),
      userScrolled: snapshot.userScrolled,
      updatedAt: snapshot.updatedAt ?? Date.now(),
      lastReadMessageId: snapshot.lastReadMessageId,
      unreadCount: snapshot.unreadCount,
      targetMessageId: snapshot.targetMessageId,
      turnIndex: snapshot.turnIndex,
    });
  }

  /**
   * Retrieve the saved scroll snapshot for a specific conversation session.
   * Touches the item to keep it fresh in the LRU order.
   */
  public get(conversationId: string | undefined | null): SessionScrollSnapshot | undefined {
    if (!conversationId) return undefined;
    const snapshot = this.snapshots.get(conversationId);
    if (!snapshot) return undefined;

    // Refresh LRU order
    this.snapshots.delete(conversationId);
    this.snapshots.set(conversationId, snapshot);
    return snapshot;
  }

  /**
   * Clear the saved scroll state for a single conversation session.
   */
  public clear(conversationId: string | undefined | null): void {
    if (!conversationId) return;
    this.snapshots.delete(conversationId);
  }

  /**
   * Clear all recorded session scroll snapshots (e.g. on user logout / full reset).
   */
  public clearAll(): void {
    this.snapshots.clear();
  }

  /**
   * Number of tracked conversation scroll positions.
   */
  public size(): number {
    return this.snapshots.size;
  }
}

export const sessionScrollRegistry = new SessionScrollRegistry();
emitter.on('conversation.deleted', (conversationId) => {
  sessionScrollRegistry.clear(conversationId);
});
