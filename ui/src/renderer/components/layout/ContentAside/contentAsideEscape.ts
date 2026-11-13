const isInsideOverlay = (node: Element | EventTarget | null): boolean => {
  if (typeof Element === 'undefined' || !(node instanceof Element)) return false;
  return Boolean(node.closest('[role="dialog"], [role="alertdialog"], .arco-modal, .arco-modal-wrapper'));
};

/** Escape closes the aside only when no higher surface owns the key or focus. */
export const shouldCloseAsideOnEscape = (
  event: Pick<KeyboardEvent, 'key' | 'defaultPrevented' | 'target'>,
  activeElement: Element | null,
): boolean => {
  if (event.key !== 'Escape') return false;
  if (event.defaultPrevented) return false;
  if (isInsideOverlay(event.target) || isInsideOverlay(activeElement)) return false;
  return true;
};
