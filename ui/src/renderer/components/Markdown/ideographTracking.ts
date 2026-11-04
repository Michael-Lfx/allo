/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { createElement, Fragment, type ReactNode } from 'react';

/**
 * Ideographs and fullwidth characters, not Latin letters.
 * Letter-spacing on a whole message stretches English words; tracking belongs
 * only on these runs.
 */
const IDEOGRAPH_SOURCE =
  '[\\u3400-\\u4dbf\\u4e00-\\u9fff\\uf900-\\ufaff\\u3000-\\u303f\\uff00-\\uffef\\u3040-\\u309f\\u30a0-\\u30ff\\u31f0-\\u31ff]';

const HAS_IDEOGRAPH = new RegExp(IDEOGRAPH_SOURCE, 'u');

const SKIP_TAGS = new Set(['code', 'pre', 'script', 'style', 'svg', 'math']);

export type IdeographRun = {
  ideograph: boolean;
  text: string;
};

type HastNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: { className?: string[] };
  children?: HastNode[];
};

export function splitIdeographRuns(text: string): IdeographRun[] {
  if (!text || !HAS_IDEOGRAPH.test(text)) {
    return text ? [{ ideograph: false, text }] : [];
  }

  const parts = text.split(new RegExp(`(${IDEOGRAPH_SOURCE}+)`, 'gu'));
  const runs: IdeographRun[] = [];
  for (const part of parts) {
    if (!part) continue;
    runs.push({ ideograph: HAS_IDEOGRAPH.test(part), text: part });
  }
  return runs;
}

export function renderIdeographTrackedText(text: string): ReactNode {
  const runs = splitIdeographRuns(text);
  if (runs.length === 0) return text;
  if (runs.length === 1 && !runs[0].ideograph) return text;

  return runs.map((run, index) =>
    run.ideograph
      ? createElement('span', { key: index, className: 'message-cjk' }, run.text)
      : createElement(Fragment, { key: index }, run.text)
  );
}

function replaceIdeographText(node: HastNode, skip: boolean): void {
  const children = node.children;
  if (!children) return;

  const skipChildren = skip || (node.tagName != null && SKIP_TAGS.has(node.tagName));
  const next: HastNode[] = [];

  for (const child of children) {
    if (!skipChildren && child.type === 'text' && typeof child.value === 'string' && HAS_IDEOGRAPH.test(child.value)) {
      for (const run of splitIdeographRuns(child.value)) {
        if (run.ideograph) {
          next.push({
            type: 'element',
            tagName: 'span',
            properties: { className: ['message-cjk'] },
            children: [{ type: 'text', value: run.text }],
          });
        } else {
          next.push({ type: 'text', value: run.text });
        }
      }
      continue;
    }

    replaceIdeographText(child, skipChildren);
    next.push(child);
  }

  node.children = next;
}

/** Wrap CJK runs so conversation tracking does not apply to Latin words. */
export function rehypeIdeographTracking() {
  return (tree: unknown) => {
    replaceIdeographText(tree as HastNode, false);
  };
}
