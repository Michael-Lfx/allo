const NAVIGATION_KEYS = new Set(['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ']);

/** One cancellable scroll in the billing container, independent of window scroll. */
export function scrollBillingTo(container: HTMLElement, target: HTMLElement, reducedMotion: boolean): () => void {
  const start = container.scrollTop;
  const destination = Math.max(0, Math.min(container.scrollHeight - container.clientHeight,
    target.getBoundingClientRect().top - container.getBoundingClientRect().top + start - 24));
  const finish = () => target.focus({ preventScroll: true });
  if (reducedMotion || Math.abs(destination - start) < 1) {
    container.scrollTop = destination;
    finish();
    return () => {};
  }
  const duration = Math.min(450, Math.max(280, Math.abs(destination - start) * .25));
  const began = performance.now();
  // Navigation keys must interrupt even while focus still sits on the sidebar
  // entry, so they are watched at page level; pointer gestures stay scoped to
  // the scroll container itself.
  const page = container.ownerDocument?.defaultView ?? null;
  let frame = 0;
  const cancel = () => {
    cancelAnimationFrame(frame);
    container.removeEventListener('wheel', interrupt);
    container.removeEventListener('touchstart', interrupt);
    container.removeEventListener('pointerdown', interrupt);
    page?.removeEventListener('keydown', onKeyDown);
  };
  const interrupt = () => cancel();
  const onKeyDown = (event: Event) => {
    if (typeof KeyboardEvent !== 'undefined' && event instanceof KeyboardEvent && !NAVIGATION_KEYS.has(event.key)) return;
    cancel();
  };
  container.addEventListener('wheel', interrupt, { passive: true });
  container.addEventListener('touchstart', interrupt, { passive: true });
  container.addEventListener('pointerdown', interrupt, { passive: true });
  page?.addEventListener('keydown', onKeyDown, { passive: true });
  const step = (now: number) => {
    const t = Math.min(1, (now - began) / duration);
    container.scrollTop = start + (destination - start) * (1 - Math.pow(1 - t, 5));
    if (t < 1) frame = requestAnimationFrame(step);
    else { cancel(); finish(); }
  };
  frame = requestAnimationFrame(step);
  return cancel;
}
