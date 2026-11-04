/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { rehypeIdeographTracking, splitIdeographRuns } from './ideographTracking';

describe('ideograph tracking', () => {
  test('leaves Latin words as a single untracked run', () => {
    expect(splitIdeographRuns('Cinematic premium sports-tech, 35mm')).toEqual([
      { ideograph: false, text: 'Cinematic premium sports-tech, 35mm' },
    ]);
  });

  test('splits ideographs away from surrounding words', () => {
    expect(splitIdeographRuns('a pearl-white 珍珠白 earbud')).toEqual([
      { ideograph: false, text: 'a pearl-white ' },
      { ideograph: true, text: '珍珠白' },
      { ideograph: false, text: ' earbud' },
    ]);
  });

  test('wraps only ideograph text and leaves code untouched', () => {
    const tree = {
      type: 'root',
      children: [
        {
          type: 'element',
          tagName: 'p',
          children: [{ type: 'text', value: 'Cinematic 珍珠白 premium' }],
        },
        {
          type: 'element',
          tagName: 'code',
          children: [{ type: 'text', value: '珍珠白' }],
        },
      ],
    };

    rehypeIdeographTracking()(tree);

    expect(tree.children[0].children).toEqual([
      { type: 'text', value: 'Cinematic ' },
      {
        type: 'element',
        tagName: 'span',
        properties: { className: ['message-cjk'] },
        children: [{ type: 'text', value: '珍珠白' }],
      },
      { type: 'text', value: ' premium' },
    ]);
    expect(tree.children[1].children).toEqual([{ type: 'text', value: '珍珠白' }]);
  });
});
