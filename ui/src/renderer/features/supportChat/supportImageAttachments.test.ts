/**
 * Shared image validation/cleanup contract used by both support surfaces.
 */

import { describe, expect, test } from 'bun:test';
import {
  createSupportImagePreviewId,
  isAcceptedSupportImage,
  MAX_SUPPORT_IMAGE_BYTES,
  MAX_SUPPORT_IMAGES,
  revokeSupportImagePreview,
  selectSupportImagePreviews,
  SUPPORT_IMAGE_ACCEPT,
} from './supportImageAttachments';

const imageFile = (name: string, type: string, size = 16): File =>
  new File([new Uint8Array(size)], name, { type, lastModified: 123 });

describe('support image attachments', () => {
  test('shares the product limits and accepts only PNG/JPG images', () => {
    expect(MAX_SUPPORT_IMAGES).toBe(4);
    expect(MAX_SUPPORT_IMAGE_BYTES).toBe(5 * 1024 * 1024);
    expect(SUPPORT_IMAGE_ACCEPT).toBe('image/png,image/jpeg,.png,.jpg,.jpeg');
    expect(isAcceptedSupportImage(imageFile('screen.PNG', ''))).toBe(true);
    expect(isAcceptedSupportImage(imageFile('screen.jpg', 'image/jpeg'))).toBe(true);
    expect(isAcceptedSupportImage(imageFile('screen.gif', 'image/gif'))).toBe(false);
    expect(isAcceptedSupportImage(imageFile('screen.txt', 'image/png'))).toBe(true);
  });

  test('creates distinct preview ids and releases object URLs through one helper', () => {
    const file = imageFile('screen.png', 'image/png');
    const first = createSupportImagePreviewId(file);
    const second = createSupportImagePreviewId(file);
    expect(first).not.toBe(second);
    expect(first).toContain('screen.png-16-123');

    const calls: string[] = [];
    const original = URL.revokeObjectURL;
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: (url: string) => calls.push(url),
    });
    try {
      revokeSupportImagePreview({ id: first, url: 'blob:test-preview', file });
    } finally {
      Object.defineProperty(URL, 'revokeObjectURL', {
        configurable: true,
        value: original,
      });
    }
    expect(calls).toEqual(['blob:test-preview']);
  });

  test('applies one selection policy and creates previews only for accepted files', () => {
    const created: string[] = [];
    const original = URL.createObjectURL;
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: (file: File) => {
        const url = `blob:${file.name}`;
        created.push(url);
        return url;
      },
    });
    try {
      const result = selectSupportImagePreviews(
        [
          imageFile('first.png', 'image/png'),
          imageFile('bad.gif', 'image/gif'),
          imageFile('second.jpg', 'image/jpeg'),
        ],
        2
      );

      expect(result.items).toHaveLength(2);
      expect(result.items[0]?.file.name).toBe('first.png');
      expect(result.items[1]?.file.name).toBe('second.jpg');
      expect(result.rejected).toBe(true);
      expect(result.truncated).toBe(false);
      expect(created).toEqual(['blob:first.png', 'blob:second.jpg']);
    } finally {
      Object.defineProperty(URL, 'createObjectURL', {
        configurable: true,
        value: original,
      });
    }
  });
});
