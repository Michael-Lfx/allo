/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IMessageAcpToolCall, IMessageToolCall, IMessageToolGroup } from '@/common/chat/chatLib';
import type { PersistedToolArtifact } from '@/common/types/platform/acpTypes';
import type { MessageId } from '@/common/types/ids';

export type StreamImageToolMessage = IMessageToolCall | IMessageAcpToolCall | IMessageToolGroup;

export interface StreamImageCarrier {
  transcriptIndex: number;
  turnId?: MessageId;
  sourceMessageIds: readonly MessageId[];
  createdAt: number;
  artifacts: readonly PersistedToolArtifact[];
}

/** One committed image receipt, pinned where its tool message first appeared. */
export interface PinnedStreamImage {
  id: string;
  artifactId: string;
  transcriptIndex: number;
  turnId?: MessageId;
  sourceMessageIds: MessageId[];
  createdAt: number;
  path: string;
  relativePath: string;
  fileName: string;
  mimeType: string;
  sizeBytes: number;
  sha256: string;
}

export interface TimelinePlacementRow {
  id: string;
  /**
   * Transcript index of the message this row represents.
   * Synthetic rows (the coalesced process disclosure) omit it so they stay
   * put and cannot pull a pinned image back into the disclosure.
   */
  transcriptIndex?: number;
  /**
   * `after-text`: assistant reply. Images from earlier in the turn render
   * after this text, not in front of it.
   * `turn-boundary`: the next user message. Images still waiting from the
   * previous turn render before it.
   */
  imageAnchor?: 'after-text' | 'turn-boundary';
}

const isCommittedImageArtifact = (artifact: PersistedToolArtifact): boolean =>
  artifact.kind === 'image' && artifact.mime_type.toLowerCase().startsWith('image/');

const fileNameFromPath = (relativePath: string): string => {
  const name = relativePath.replace(/\\/g, '/').split('/').filter(Boolean).at(-1);
  return name && name.length > 0 ? name : relativePath;
};

const normalizePathKey = (value: string): string =>
  value.trim().replace(/\\/g, '/').replace(/\/{2,}/g, '/').replace(/^\.\//, '').toLowerCase();

export const committedImageArtifactsFromTools = (
  messages: readonly StreamImageToolMessage[]
): PersistedToolArtifact[] => {
  const artifacts: PersistedToolArtifact[] = [];
  for (const message of messages) {
    if (message.type === 'tool_call') {
      for (const artifact of message.content.artifacts ?? []) {
        if (isCommittedImageArtifact(artifact)) artifacts.push(artifact);
      }
      continue;
    }
    if (message.type === 'acp_tool_call') {
      for (const item of message.content.update?.content ?? []) {
        if (item.type === 'artifact' && isCommittedImageArtifact(item.artifact)) {
          artifacts.push(item.artifact);
        }
      }
    }
  }
  return artifacts;
};

/**
 * Image receipts in first-seen order. A repeated artifact id keeps the
 * earlier transcript index; later copies do not move it.
 * Uncommitted tool rows reach this function with an empty `artifacts` array.
 */
export const collectPinnedStreamImages = (carriers: readonly StreamImageCarrier[]): PinnedStreamImage[] => {
  const seen = new Set<string>();
  const images: PinnedStreamImage[] = [];

  for (const carrier of carriers) {
    for (const artifact of carrier.artifacts) {
      if (!isCommittedImageArtifact(artifact) || seen.has(artifact.id)) continue;
      seen.add(artifact.id);
      images.push({
        id: `stream-image-${artifact.id}`,
        artifactId: artifact.id,
        transcriptIndex: carrier.transcriptIndex,
        ...(carrier.turnId ? { turnId: carrier.turnId } : {}),
        sourceMessageIds: [...carrier.sourceMessageIds],
        createdAt: carrier.createdAt,
        path: artifact.path,
        relativePath: artifact.relative_path,
        fileName: fileNameFromPath(artifact.relative_path),
        mimeType: artifact.mime_type,
        sizeBytes: artifact.size_bytes,
        sha256: artifact.sha256,
      });
    }
  }

  return images;
};

/**
 * Put pinned images after the reply text of their turn.
 * The process disclosure stays where it is. A later user message closes the
 * previous turn, so its images cannot slide under the next question.
 * Images that have no following reply yet stay at the end of the current list.
 */
export const placePinnedStreamImages = <Row extends TimelinePlacementRow>(
  rows: readonly Row[],
  images: readonly PinnedStreamImage[],
  toRow: (image: PinnedStreamImage) => Row
): Row[] => {
  const pending = images
    .map((image, order) => ({ image, order }))
    .sort((left, right) => left.image.transcriptIndex - right.image.transcriptIndex || left.order - right.order)
    .map((entry) => entry.image);

  const placed: Row[] = [];
  let cursor = 0;

  const flushBefore = (transcriptIndex: number) => {
    while (cursor < pending.length && pending[cursor].transcriptIndex < transcriptIndex) {
      placed.push(toRow(pending[cursor]));
      cursor += 1;
    }
  };

  const flushThrough = (transcriptIndex: number) => {
    while (cursor < pending.length && pending[cursor].transcriptIndex <= transcriptIndex) {
      placed.push(toRow(pending[cursor]));
      cursor += 1;
    }
  };

  for (const row of rows) {
    if (row.imageAnchor === 'turn-boundary' && row.transcriptIndex !== undefined) {
      flushBefore(row.transcriptIndex);
      placed.push(row);
      continue;
    }
    placed.push(row);
    if (row.imageAnchor === 'after-text' && row.transcriptIndex !== undefined) {
      flushThrough(row.transcriptIndex);
    }
  }

  while (cursor < pending.length) {
    placed.push(toRow(pending[cursor]));
    cursor += 1;
  }

  return placed;
};

export const isPinnedStreamImageDeliverable = (
  item: { relativePath: string; absolutePath?: string },
  images: readonly PinnedStreamImage[]
): boolean => {
  const relative = normalizePathKey(item.relativePath);
  const absolute = item.absolutePath ? normalizePathKey(item.absolutePath) : undefined;
  return images.some((image) => {
    if (normalizePathKey(image.relativePath) === relative) return true;
    return absolute !== undefined && normalizePathKey(image.path) === absolute;
  });
};

export const omitPinnedStreamImageDeliverables = <T extends { relativePath: string; absolutePath?: string }>(
  items: readonly T[],
  images: readonly PinnedStreamImage[]
): T[] => items.filter((item) => !isPinnedStreamImageDeliverable(item, images));
