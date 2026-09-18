/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

type MarkdownNodeLike = {
  type?: unknown;
  tagName?: unknown;
  children?: unknown;
};

function asMarkdownNode(value: unknown): MarkdownNodeLike | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }

  return value as MarkdownNodeLike;
}

function containsNodeType(value: unknown, type: string): boolean {
  const node = asMarkdownNode(value);
  if (!node) {
    return false;
  }

  if (node.type === type || node.tagName === type) {
    return true;
  }

  return Array.isArray(node.children)
    ? node.children.some((child) => containsNodeType(child, type))
    : false;
}

function containsFencedCode(value: unknown): boolean {
  const node = asMarkdownNode(value);
  if (!node) {
    return false;
  }

  // mdast exposes fenced code as `code`; hast exposes it as `pre > code`.
  if (node.type === 'code' || node.tagName === 'pre') {
    return true;
  }

  return Array.isArray(node.children) ? node.children.some(containsFencedCode) : false;
}

/**
 * Prevent nested collapse controls for blockquotes that contain another
 * blockquote or a fenced code block.
 */
export function shouldCollapseMarkdownBlockquote(node: unknown): boolean {
  const blockquote = asMarkdownNode(node);
  if (!blockquote || !Array.isArray(blockquote.children)) {
    return false;
  }

  return !blockquote.children.some(
    (child) => containsNodeType(child, 'blockquote') || containsFencedCode(child),
  );
}
