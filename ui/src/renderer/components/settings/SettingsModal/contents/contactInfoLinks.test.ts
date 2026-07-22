/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('public contact links', () => {
  test('does not expose legacy nomifun website or GitHub contact surfaces', () => {
    const aboutSource = readSource(new URL('./AboutModalContent.tsx', import.meta.url));
    const contactSource = readSource(new URL('./FeedbackReportModal.tsx', import.meta.url));
    const combined = `${aboutSource}\n${contactSource}`;

    for (const target of [
      'https://www.nomifun.com',
      'https://www.nomifun.com/contact',
      'https://github.com/nomifun/nomifun-tauri/issues',
      'https://github.com/nomifun/nomifun-tauri/releases',
    ]) {
      expect(combined.includes(target)).toBe(false);
    }
  });

  test('keeps the Contact modal visually quiet instead of rendering chunky cards', () => {
    const contactSource = readSource(new URL('./FeedbackReportModal.tsx', import.meta.url));

    expect(contactSource.includes("import CopyIconButton from '@/renderer/components/base/CopyIconButton'")).toBe(false);
    expect(contactSource.includes("<Info theme='outline' size='28' />")).toBe(false);
    expect(contactSource.includes("bg-fill-2 px-12px py-10px")).toBe(false);
    expect(contactSource.includes('>↗<')).toBe(false);
  });

  test('does not render external release or website shortcuts in the native update modal', () => {
    const updateModalSource = readSource(new URL('../../UpdateModal.tsx', import.meta.url));

    expect(updateModalSource.includes('https://www.nomifun.com')).toBe(false);
    expect(updateModalSource.includes('github.com/nomifun')).toBe(false);
    expect(updateModalSource.includes('openReleasePage')).toBe(false);
    expect(updateModalSource.includes('openProductWebsite')).toBe(false);
  });
});
