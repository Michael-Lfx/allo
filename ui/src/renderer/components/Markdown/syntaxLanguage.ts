/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Grammars registered by `./SyntaxHighlighter` (Highlight.js light adapter). */
const SUPPORTED_LANGUAGES = new Set([
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
  'plaintext',
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
  'text',
  'txt',
  'plain',
  'mermaid',
]);

const LANGUAGE_ALIASES: Record<string, string> = {
  'c#': 'csharp',
  'c++': 'cpp',
  cjs: 'javascript',
  console: 'text',
  cs: 'csharp',
  docker: 'dockerfile',
  error: 'text',
  htm: 'xml',
  html: 'xml',
  js: 'javascript',
  json5: 'json',
  jsx: 'javascript',
  kt: 'kotlin',
  kts: 'kotlin',
  log: 'text',
  math: 'latex',
  md: 'markdown',
  mjs: 'javascript',
  patch: 'diff',
  ps1: 'powershell',
  py: 'python',
  rb: 'ruby',
  rs: 'rust',
  sh: 'bash',
  'shell-session': 'bash',
  stack: 'text',
  svg: 'xml',
  tex: 'latex',
  ts: 'typescript',
  tsx: 'typescript',
  yml: 'yaml',
  zsh: 'bash',
};

/**
 * Resolve a Markdown fence label to a grammar we explicitly ship.
 *
 * Never auto-detect an unknown label. Error messages commonly use informal
 * fences such as `log`, `console`, or `error`; treating those as plain text is
 * both more accurate and safer than running every registered grammar.
 */
export const resolveSyntaxLanguage = (language: string | undefined): string => {
  const normalized = language?.trim().toLowerCase() || 'text';
  const aliased = LANGUAGE_ALIASES[normalized] ?? normalized;
  return SUPPORTED_LANGUAGES.has(aliased) ? aliased : 'text';
};
