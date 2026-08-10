import { describe, expect, test } from 'bun:test';
import {
  MAX_IMAGE_ATTACHMENTS,
  admitImageAttachments,
  hasTooManyImageAttachments,
} from './imageAttachments';

const imagePaths = (count: number): string[] =>
  Array.from({ length: count }, (_, index) => `C:/images/${index + 1}.png`);

describe('image attachment admission', () => {
  test('admits at most ten distinct images while preserving non-image files', () => {
    const admission = admitImageAttachments([], [...imagePaths(MAX_IMAGE_ATTACHMENTS + 1), 'C:/notes/readme.md']);

    expect(admission.acceptedPaths).toEqual([...imagePaths(MAX_IMAGE_ATTACHMENTS), 'C:/notes/readme.md']);
    expect(admission.rejectedImageCount).toBe(1);
  });

  test('does not spend another image slot for an existing or repeated path', () => {
    const existing = imagePaths(MAX_IMAGE_ATTACHMENTS - 1);
    const admission = admitImageAttachments(existing, [existing[0], 'C:/images/final.PNG', 'C:/images/final.PNG']);

    expect(admission.acceptedPaths).toEqual(['C:/images/final.PNG']);
    expect(admission.rejectedImageCount).toBe(0);
  });

  test('detects an oversized initial-message handoff before it reaches the runtime', () => {
    expect(hasTooManyImageAttachments(imagePaths(MAX_IMAGE_ATTACHMENTS + 1))).toBe(true);
    expect(hasTooManyImageAttachments([...imagePaths(MAX_IMAGE_ATTACHMENTS), 'C:/notes/readme.md'])).toBe(false);
  });
});
