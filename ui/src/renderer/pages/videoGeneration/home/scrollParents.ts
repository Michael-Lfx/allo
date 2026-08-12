/** Collect overflow-scrolling ancestors (not including window). */
export function getScrollParents(node: HTMLElement | null): HTMLElement[] {
  const parents: HTMLElement[] = [];
  let el = node?.parentElement ?? null;
  while (el) {
    const { overflow, overflowX, overflowY } = window.getComputedStyle(el);
    const value = `${overflow}${overflowX}${overflowY}`;
    if (/(auto|scroll|overlay)/.test(value)) {
      parents.push(el);
    }
    el = el.parentElement;
  }
  return parents;
}
