/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

type MdastNode = {
  type: string;
  value?: string;
  children?: MdastNode[];
};

const BR_HTML = /^<br\s*\/?\s*>$/i;

const visit = (node: MdastNode): void => {
  const children = node.children;
  if (!children) return;

  for (let index = 0; index < children.length; index += 1) {
    const child = children[index];
    if (child.type === 'html' && typeof child.value === 'string' && BR_HTML.test(child.value.trim())) {
      children[index] = { type: 'break' };
      continue;
    }
    visit(child);
  }
};

/**
 * Models often put `<br>` inside GFM table cells because those cells cannot
 * contain markdown newlines. Without `rehype-raw`, react-markdown escapes the
 * tag into visible text. Promote only `<br>` HTML nodes to mdast `break`s so
 * conversation tables get a real line break without opening raw HTML.
 */
export function remarkSafeBreaks() {
  return (tree: MdastNode): void => {
    visit(tree);
  };
}
