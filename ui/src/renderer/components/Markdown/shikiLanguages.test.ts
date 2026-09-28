/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { resolveSyntaxLanguage } from './syntaxLanguage';
import { hasShikiLangLoader, isPlainShikiLang, toShikiLangId } from './shikiLanguages';

const FENCES = [
  'bash',
  'c',
  'cpp',
  'csharp',
  'css',
  'diff',
  'dockerfile',
  'go',
  'ini',
  'java',
  'javascript',
  'json',
  'kotlin',
  'latex',
  'lua',
  'makefile',
  'markdown',
  'php',
  'powershell',
  'python',
  'ruby',
  'rust',
  'scss',
  'shell',
  'sql',
  'swift',
  'typescript',
  'vbnet',
  'xml',
  'yaml',
  'mermaid',
  'js',
  'ts',
  'py',
  'html',
  'tsx',
  'jsx',
  'toml',
  'vue',
  'graphql',
  'nginx',
  'proto',
  'http',
  'yml',
  'log',
];

describe('Shiki language mapping', () => {
  test('covers every syntaxLanguage resolver result', () => {
    for (const fence of FENCES) {
      const resolved = resolveSyntaxLanguage(fence);
      expect(hasShikiLangLoader(resolved)).toBe(true);
    }
  });

  test('maps plain and vbnet fences onto Shiki ids', () => {
    expect(isPlainShikiLang(resolveSyntaxLanguage('log'))).toBe(true);
    expect(toShikiLangId(resolveSyntaxLanguage('log'))).toBe('plaintext');
    expect(toShikiLangId('vbnet')).toBe('vb');
    expect(toShikiLangId('typescript')).toBe('typescript');
    expect(toShikiLangId('tsx')).toBe('tsx');
    expect(toShikiLangId('html')).toBe('html');
  });
});
