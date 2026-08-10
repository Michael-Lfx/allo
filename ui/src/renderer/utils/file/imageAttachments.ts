import { imageExts } from '@/renderer/services/FileService';

/** Matches the Nomi runtime's maximum number of distinct image inputs per message. */
export const MAX_IMAGE_ATTACHMENTS = 10;

export const isImageAttachment = (path: string): boolean => {
  const normalized = path.toLowerCase();
  return imageExts.some((extension) => normalized.endsWith(extension));
};

export type ImageAttachmentAdmission = {
  acceptedPaths: string[];
  rejectedImageCount: number;
};

/**
 * Accept new paths while reserving at most ten distinct image attachments for
 * a message. Existing and repeated paths are ignored rather than re-counted.
 */
export const admitImageAttachments = (
  existingPaths: readonly string[],
  candidatePaths: readonly string[]
): ImageAttachmentAdmission => {
  const known = new Set(existingPaths);
  let imageCount = [...known].filter(isImageAttachment).length;
  const acceptedPaths: string[] = [];
  let rejectedImageCount = 0;

  for (const path of candidatePaths) {
    if (known.has(path)) continue;
    if (isImageAttachment(path) && imageCount >= MAX_IMAGE_ATTACHMENTS) {
      rejectedImageCount += 1;
      continue;
    }
    known.add(path);
    acceptedPaths.push(path);
    if (isImageAttachment(path)) imageCount += 1;
  }

  return { acceptedPaths, rejectedImageCount };
};

export const hasTooManyImageAttachments = (paths: readonly string[]): boolean =>
  new Set(paths.filter(isImageAttachment)).size > MAX_IMAGE_ATTACHMENTS;
