/**
 * Strip `MEDIA:<absolute path>` delivery tags from assistant text.
 *
 * Image tools ask the model to echo a local path so a host can find the file.
 * The conversation already shows that image from the committed artifact, so
 * the path is not part of the answer.
 */

const MEDIA_DELIVERY_TAG =
  /(?:^[ \t]*|[ \t])MEDIA:(?:[A-Za-z]:[\\/]|\\\\|\/)\S*/gm;

export function hasMediaDeliveryTag(content: string): boolean {
  if (!content || !content.includes('MEDIA:')) return false;
  MEDIA_DELIVERY_TAG.lastIndex = 0;
  return MEDIA_DELIVERY_TAG.test(content);
}

export function stripMediaDeliveryTags(content: string): string {
  if (!hasMediaDeliveryTag(content)) return content;
  MEDIA_DELIVERY_TAG.lastIndex = 0;
  return content
    .replace(MEDIA_DELIVERY_TAG, '')
    .replace(/[ \t]+$/gm, '')
    .replace(/\n{3,}/g, '\n\n');
}
