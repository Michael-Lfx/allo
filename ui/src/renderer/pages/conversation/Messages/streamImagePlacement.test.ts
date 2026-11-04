/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { PersistedToolArtifact } from '@/common/types/platform/acpTypes';
import { parseMessageId, parsePersistedArtifactId } from '@/common/types/ids';
import type { IMessageAcpToolCall, IMessageToolCall } from '@/common/chat/chatLib';
import {
  collectPinnedStreamImages,
  committedImageArtifactsFromTools,
  omitPinnedStreamImageDeliverables,
  placePinnedStreamImages,
  type PinnedStreamImage,
  type StreamImageCarrier,
  type TimelinePlacementRow,
} from './streamImagePlacement';

const TURN = parseMessageId('0190f5fe-7c00-7a00-8000-000000000001');
const MSG = parseMessageId('0190f5fe-7c00-7a00-8000-000000000021');

const imageArtifact = (id: string, fileName: string, overrides: Partial<PersistedToolArtifact> = {}): PersistedToolArtifact => ({
  id: parsePersistedArtifactId(id),
  kind: 'image',
  mime_type: 'image/jpeg',
  path: `C:/workspace/nomifun-artifacts/${fileName}`,
  relative_path: `nomifun-artifacts/${fileName}`,
  size_bytes: 1024,
  sha256: 'a'.repeat(64),
  ...overrides,
});

const carrier = (
  transcriptIndex: number,
  artifacts: PersistedToolArtifact[],
  overrides: Partial<StreamImageCarrier> = {}
): StreamImageCarrier => ({
  transcriptIndex,
  turnId: TURN,
  sourceMessageIds: [MSG],
  createdAt: transcriptIndex,
  artifacts,
  ...overrides,
});

const row = (
  id: string,
  transcriptIndex?: number,
  imageAnchor?: TimelinePlacementRow['imageAnchor']
): TimelinePlacementRow => ({
  id,
  ...(transcriptIndex === undefined ? {} : { transcriptIndex }),
  ...(imageAnchor ? { imageAnchor } : {}),
});

const placeIds = (rows: TimelinePlacementRow[], images: PinnedStreamImage[]): string[] =>
  placePinnedStreamImages(rows, images, (image) => ({
    id: image.id,
    transcriptIndex: image.transcriptIndex,
  })).map((entry) => entry.id);

describe('committedImageArtifactsFromTools', () => {
  test('reads a committed tool_call image and an ACP image block, and skips an empty receipt', () => {
    const cat = imageArtifact('0190f5fe-7c00-7a00-8000-000000000031', 'cat.jpg');
    const toolCall = {
      type: 'tool_call',
      content: { artifacts: [cat] },
    } as IMessageToolCall;
    const emptyCall = {
      type: 'tool_call',
      content: { artifacts: [] },
    } as IMessageToolCall;
    const acpCall = {
      type: 'acp_tool_call',
      content: { update: { content: [{ type: 'artifact', artifact: cat }] } },
    } as IMessageAcpToolCall;

    expect(committedImageArtifactsFromTools([toolCall, emptyCall, acpCall])).toEqual([cat, cat]);
  });
});

describe('collectPinnedStreamImages', () => {
  test('ignores an uncommitted tool row and non-image receipts', () => {
    const images = collectPinnedStreamImages([
      carrier(1, []),
      carrier(2, [
        imageArtifact('0190f5fe-7c00-7a00-8000-000000000031', 'notes.txt', {
          kind: 'file',
          mime_type: 'text/plain',
          relative_path: 'notes.txt',
          path: 'C:/workspace/notes.txt',
        }),
      ]),
    ]);

    expect(images).toEqual([]);
  });

  test('pins the first time an image receipt appears', () => {
    const artifact = imageArtifact('0190f5fe-7c00-7a00-8000-000000000031', 'cat.jpg');
    const images = collectPinnedStreamImages([
      carrier(2, [artifact]),
      carrier(6, [artifact]),
    ]);

    expect(images).toHaveLength(1);
    expect(images[0]).toMatchObject({
      id: `stream-image-${artifact.id}`,
      transcriptIndex: 2,
      fileName: 'cat.jpg',
      relativePath: 'nomifun-artifacts/cat.jpg',
    });
  });
});

describe('placePinnedStreamImages', () => {
  const cat = imageArtifact('0190f5fe-7c00-7a00-8000-000000000031', 'cat.jpg');
  const dog = imageArtifact('0190f5fe-7c00-7a00-8000-000000000032', 'dog.jpg');

  test('places an image after the reply text, not in front of it', () => {
    const images = collectPinnedStreamImages([carrier(2, [cat])]);
    expect(
      placeIds(
        [row('user', 0, 'turn-boundary'), row('disclosure'), row('answer', 3, 'after-text')],
        images
      )
    ).toEqual(['user', 'disclosure', 'answer', images[0].id]);
  });

  test('places an image after an answer that already appeared', () => {
    const images = collectPinnedStreamImages([carrier(2, [cat])]);
    expect(
      placeIds(
        [row('user', 0, 'turn-boundary'), row('disclosure'), row('answer', 1, 'after-text')],
        images
      )
    ).toEqual(['user', 'disclosure', 'answer', images[0].id]);
  });

  test('keeps two images in transcript order after the reply', () => {
    const images = collectPinnedStreamImages([carrier(4, [dog]), carrier(2, [cat])]);
    expect(
      placeIds(
        [row('user', 0, 'turn-boundary'), row('disclosure'), row('answer', 5, 'after-text')],
        images
      )
    ).toEqual(['user', 'disclosure', 'answer', `stream-image-${cat.id}`, `stream-image-${dog.id}`]);
  });

  test('a later reply stays ahead of the image', () => {
    const images = collectPinnedStreamImages([carrier(2, [cat])]);
    const before = placeIds(
      [row('user', 0, 'turn-boundary'), row('answer', 1, 'after-text')],
      images
    );
    const after = placeIds(
      [row('user', 0, 'turn-boundary'), row('answer', 1, 'after-text'), row('later', 4, 'after-text')],
      images
    );

    expect(before).toEqual(['user', 'answer', images[0].id]);
    expect(after).toEqual(['user', 'answer', 'later', images[0].id]);
  });

  test('the next question keeps the previous turn’s images above it', () => {
    const images = collectPinnedStreamImages([carrier(2, [cat]), carrier(6, [dog])]);
    expect(
      placeIds(
        [
          row('user', 0, 'turn-boundary'),
          row('answer', 3, 'after-text'),
          row('user-2', 5, 'turn-boundary'),
          row('answer-2', 8, 'after-text'),
        ],
        images
      )
    ).toEqual(['user', 'answer', images[0].id, 'user-2', 'answer-2', images[1].id]);
  });
});

describe('omitPinnedStreamImageDeliverables', () => {
  test('drops a shown image path and keeps other files', () => {
    const images = collectPinnedStreamImages([
      carrier(2, [imageArtifact('0190f5fe-7c00-7a00-8000-000000000031', 'cat.jpg')]),
    ]);
    const remaining = omitPinnedStreamImageDeliverables(
      [
        { relativePath: 'nomifun-artifacts\\cat.jpg', absolutePath: 'C:/workspace/nomifun-artifacts/cat.jpg' },
        { relativePath: 'notes.md' },
      ],
      images
    );

    expect(remaining.map((item) => item.relativePath)).toEqual(['notes.md']);
  });
});
