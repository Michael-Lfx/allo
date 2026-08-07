/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { splitStreamingMarkdown } from './streamingMarkdown';

describe('splitStreamingMarkdown', () => {
  test('keeps the final incomplete paragraph as lightweight text', () => {
    const result = splitStreamingMarkdown('## Done\n\nThe answer is still being written');

    expect(result.stablePrefix).toBe('## Done\n\n');
    expect(result.tail).toBe('The answer is still being written');
    expect(result.tailKind).toBe('text');
  });

  test('keeps completed fenced blocks in the stable Markdown prefix', () => {
    const result = splitStreamingMarkdown('```ts\nconst ready = true;\n```\n\nNext paragraph');

    expect(result.stablePrefix).toBe('```ts\nconst ready = true;\n```\n\n');
    expect(result.tail).toBe('Next paragraph');
    expect(result.tailKind).toBe('text');
  });

  test('turns an open fenced block into a lightweight code tail', () => {
    const result = splitStreamingMarkdown('Intro\n\n```tsx\nconst value = <Panel />');

    expect(result.stablePrefix).toBe('Intro\n\n');
    expect(result.tailKind).toBe('code');
    expect(result.codeLanguage).toBe('tsx');
    expect(result.codeContent).toBe('const value = <Panel />');
  });
});
