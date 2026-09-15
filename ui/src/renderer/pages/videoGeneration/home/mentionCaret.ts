export const HOME_IMAGE_MENTION_MENU_GAP_PX = 8;
export const HOME_IMAGE_MENTION_MENU_WIDTH_PX = 256;

const MIRROR_STYLE_PROPS = [
  'boxSizing',
  'width',
  'borderTopWidth',
  'borderRightWidth',
  'borderBottomWidth',
  'borderLeftWidth',
  'paddingTop',
  'paddingRight',
  'paddingBottom',
  'paddingLeft',
  'fontStyle',
  'fontVariant',
  'fontWeight',
  'fontStretch',
  'fontSize',
  'lineHeight',
  'fontFamily',
  'textAlign',
  'textTransform',
  'textIndent',
  'textDecoration',
  'letterSpacing',
  'wordSpacing',
  'tabSize',
  'whiteSpace',
  'wordWrap',
  'wordBreak',
] as const;

export type TextareaCaretRect = {
  left: number;
  top: number;
  bottom: number;
  height: number;
};

export function getTextareaCaretViewportRect(
  textarea: HTMLTextAreaElement,
  index: number,
): TextareaCaretRect {
  const style = window.getComputedStyle(textarea);
  const mirror = document.createElement('div');
  mirror.setAttribute('aria-hidden', 'true');
  mirror.style.position = 'absolute';
  mirror.style.visibility = 'hidden';
  mirror.style.overflow = 'hidden';
  mirror.style.top = '0';
  mirror.style.left = '-9999px';
  mirror.style.whiteSpace = 'pre-wrap';
  mirror.style.wordWrap = 'break-word';
  for (const prop of MIRROR_STYLE_PROPS) {
    mirror.style[prop] = style[prop];
  }
  mirror.style.width = `${textarea.clientWidth}px`;
  mirror.style.height = 'auto';

  const value = textarea.value;
  const at = Math.max(0, Math.min(index, value.length));
  mirror.appendChild(document.createTextNode(value.slice(0, at)));
  const marker = document.createElement('span');
  marker.textContent = value[at] || '@';
  marker.style.display = 'inline';
  marker.style.lineHeight = style.lineHeight;
  mirror.appendChild(marker);
  document.body.appendChild(mirror);

  const textareaRect = textarea.getBoundingClientRect();
  const mirrorRect = mirror.getBoundingClientRect();
  const markerRect = marker.getBoundingClientRect();
  const height = markerRect.height || parseFloat(style.lineHeight) || 24;
  const left = textareaRect.left + (markerRect.left - mirrorRect.left) - textarea.scrollLeft;
  const top = textareaRect.top + (markerRect.top - mirrorRect.top) - textarea.scrollTop;
  mirror.remove();
  return { left, top, bottom: top + height, height };
}

export function syncHomeMentionHighlightOverlay(
  textarea: HTMLTextAreaElement,
  highlight: HTMLElement,
  shell: HTMLElement,
): void {
  const style = window.getComputedStyle(textarea);
  const shellRect = shell.getBoundingClientRect();
  const textareaRect = textarea.getBoundingClientRect();
  highlight.style.left = `${textareaRect.left - shellRect.left}px`;
  highlight.style.top = `${textareaRect.top - shellRect.top}px`;
  highlight.style.width = `${textareaRect.width}px`;
  highlight.style.height = `${textareaRect.height}px`;
  for (const prop of MIRROR_STYLE_PROPS) {
    if (prop === 'width') continue;
    highlight.style[prop] = style[prop];
  }
  highlight.style.whiteSpace = 'pre-wrap';
  highlight.style.overflowWrap = 'break-word';
  highlight.scrollTop = textarea.scrollTop;
  highlight.scrollLeft = textarea.scrollLeft;
}

export function setHomeMentionTextareaFill(
  textarea: HTMLTextAreaElement,
  transparent: boolean,
): void {
  if (transparent) {
    textarea.style.setProperty('color', 'transparent', 'important');
    textarea.style.setProperty('-webkit-text-fill-color', 'transparent', 'important');
    textarea.style.setProperty('caret-color', 'var(--color-text-1)', 'important');
    return;
  }
  textarea.style.removeProperty('color');
  textarea.style.removeProperty('-webkit-text-fill-color');
  textarea.style.removeProperty('caret-color');
}

export function placeHomeImageMentionMenu(
  caret: Pick<TextareaCaretRect, 'left' | 'top' | 'bottom'>,
  viewport: { width: number; height: number },
  menu: { width: number; estimatedHeight: number },
): { left: number; top: number; width: number } {
  const width = menu.width;
  const left = Math.max(8, Math.min(caret.left, viewport.width - width - 8));
  const below = caret.bottom + HOME_IMAGE_MENTION_MENU_GAP_PX;
  const fitsBelow = below + menu.estimatedHeight <= viewport.height - 8;
  const top = fitsBelow
    ? below
    : Math.max(8, caret.top - HOME_IMAGE_MENTION_MENU_GAP_PX - menu.estimatedHeight);
  return { left, top, width };
}
