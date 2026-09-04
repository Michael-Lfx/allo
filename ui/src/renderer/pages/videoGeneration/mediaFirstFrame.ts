/** Decode a visible first frame so muted previews are not a black rectangle. */
export function seekMediaElementToFirstFrame(media: HTMLMediaElement) {
  if (media.currentTime !== 0 || !(media.duration > 0)) return;
  media.currentTime = Math.min(0.001, media.duration);
}
