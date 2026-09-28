/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import styles from './codeBlock.module.css';

const STYLE_ATTR = 'data-beautiful-ui-code-block-css';

let constructedSheet: CSSStyleSheet | null | undefined;

const cssTextForModule = (): string | null => {
  const token = styles.root;
  for (const sheet of document.styleSheets) {
    try {
      const rules = [...sheet.cssRules];
      if (!rules.some((rule) => rule.cssText.includes(token))) continue;
      return rules.map((rule) => rule.cssText).join('\n');
    } catch {
      continue;
    }
  }
  return null;
};

/** Copy Beautiful UI code-block CSS into a ShadowRoot so hashed classes resolve. */
export const adoptBeautifulUiCodeBlockCss = (node: HTMLElement): void => {
  const root = node.getRootNode();
  if (!(root instanceof ShadowRoot)) return;
  if (root.querySelector(`style[${STYLE_ATTR}]`)) return;

  if (constructedSheet === undefined) {
    const cssText = cssTextForModule();
    if (cssText && typeof CSSStyleSheet !== 'undefined' && 'replaceSync' in CSSStyleSheet.prototype) {
      constructedSheet = new CSSStyleSheet();
      constructedSheet.replaceSync(cssText);
    } else {
      constructedSheet = null;
    }
  }

  if (constructedSheet && !root.adoptedStyleSheets.includes(constructedSheet)) {
    root.adoptedStyleSheets = [...root.adoptedStyleSheets, constructedSheet];
    return;
  }

  const cssText = cssTextForModule();
  if (!cssText) return;
  const style = document.createElement('style');
  style.setAttribute(STYLE_ATTR, '');
  style.textContent = cssText;
  root.appendChild(style);
};
