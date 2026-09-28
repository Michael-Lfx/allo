/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { LanguageRegistration } from 'shiki/core';

/**
 * Current GitHub website / VS Code "GitHub Light/Dark Default" palettes.
 * The legacy `github-light` / `github-dark` themes leave types, parameters,
 * and operators uncolored in light mode.
 */
export const SHIKI_THEME_LIGHT = 'github-light-default';
export const SHIKI_THEME_DARK = 'github-dark-default';

const PLAIN_SHIKI_LANGS = new Set(['text', 'txt', 'plain', 'plaintext']);

type ShikiLangModule = LanguageRegistration[] | { default: LanguageRegistration[] };

const SHIKI_LANG_LOADERS: Record<string, () => Promise<ShikiLangModule>> = {
  bash: () => import('@shikijs/langs/bash'),
  c: () => import('@shikijs/langs/c'),
  cpp: () => import('@shikijs/langs/cpp'),
  csharp: () => import('@shikijs/langs/csharp'),
  css: () => import('@shikijs/langs/css'),
  diff: () => import('@shikijs/langs/diff'),
  dockerfile: () => import('@shikijs/langs/dockerfile'),
  go: () => import('@shikijs/langs/go'),
  graphql: () => import('@shikijs/langs/graphql'),
  html: () => import('@shikijs/langs/html'),
  http: () => import('@shikijs/langs/http'),
  ini: () => import('@shikijs/langs/ini'),
  java: () => import('@shikijs/langs/java'),
  javascript: () => import('@shikijs/langs/javascript'),
  json: () => import('@shikijs/langs/json'),
  jsx: () => import('@shikijs/langs/jsx'),
  kotlin: () => import('@shikijs/langs/kotlin'),
  latex: () => import('@shikijs/langs/latex'),
  lua: () => import('@shikijs/langs/lua'),
  makefile: () => import('@shikijs/langs/makefile'),
  markdown: () => import('@shikijs/langs/markdown'),
  mermaid: () => import('@shikijs/langs/mermaid'),
  nginx: () => import('@shikijs/langs/nginx'),
  php: () => import('@shikijs/langs/php'),
  powershell: () => import('@shikijs/langs/powershell'),
  proto: () => import('@shikijs/langs/proto'),
  python: () => import('@shikijs/langs/python'),
  ruby: () => import('@shikijs/langs/ruby'),
  rust: () => import('@shikijs/langs/rust'),
  scss: () => import('@shikijs/langs/scss'),
  shell: () => import('@shikijs/langs/shell'),
  sql: () => import('@shikijs/langs/sql'),
  swift: () => import('@shikijs/langs/swift'),
  toml: () => import('@shikijs/langs/toml'),
  tsx: () => import('@shikijs/langs/tsx'),
  typescript: () => import('@shikijs/langs/typescript'),
  vbnet: () => import('@shikijs/langs/vb'),
  vue: () => import('@shikijs/langs/vue'),
  xml: () => import('@shikijs/langs/xml'),
  yaml: () => import('@shikijs/langs/yaml'),
};

export const isPlainShikiLang = (resolved: string): boolean => PLAIN_SHIKI_LANGS.has(resolved);

export const toShikiLangId = (resolved: string): string => {
  if (isPlainShikiLang(resolved)) return 'plaintext';
  if (resolved === 'vbnet') return 'vb';
  return resolved;
};

export const getShikiLangLoader = (resolved: string): (() => Promise<ShikiLangModule>) | undefined => {
  if (isPlainShikiLang(resolved)) return undefined;
  return SHIKI_LANG_LOADERS[resolved];
};

export const hasShikiLangLoader = (resolved: string): boolean =>
  isPlainShikiLang(resolved) || Boolean(SHIKI_LANG_LOADERS[resolved]);
