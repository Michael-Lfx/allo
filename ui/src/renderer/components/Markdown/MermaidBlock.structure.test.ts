/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const mermaidSource = readFileSync(new URL('./MermaidBlock.tsx', import.meta.url), 'utf8');
const shadowSource = readFileSync(new URL('./ShadowView.tsx', import.meta.url), 'utf8');

describe('Mermaid Shadow DOM toolbar', () => {
  test('uses local semantic classes rather than document utility classes', () => {
    expect(mermaidSource.includes("className='markdown-mermaid-block'")).toBe(true);
    expect(mermaidSource.includes("className='markdown-mermaid-toolbar markdown-code-toolbar'")).toBe(true);
    expect(mermaidSource.includes("className='markdown-mermaid-segmented'")).toBe(true);
    expect(mermaidSource.includes("className='markdown-mermaid-segment'")).toBe(true);
    expect(mermaidSource.includes("className='border-0 bg-transparent p-0 cursor-pointer flex items-center'")).toBe(false);
  });

  test('defines visible segmented and keyboard states inside the Shadow DOM', () => {
    expect(shadowSource.includes('.markdown-mermaid-segment[aria-pressed=\'true\']')).toBe(true);
    expect(shadowSource.includes('.markdown-mermaid-segment:hover')).toBe(true);
    expect(shadowSource.includes('.markdown-mermaid-segment:focus-visible')).toBe(true);
    expect(shadowSource.includes('.markdown-mermaid-block:hover .markdown-mermaid-toolbar')).toBe(true);
    expect(shadowSource.includes('.markdown-mermaid-loading')).toBe(true);
    expect(shadowSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
