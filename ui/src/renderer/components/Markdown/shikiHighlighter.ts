/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { createHighlighterCore } from 'shiki/core';
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript';
import githubDarkDefault from '@shikijs/themes/github-dark-default';
import githubLightDefault from '@shikijs/themes/github-light-default';
import {
  SHIKI_THEME_DARK,
  SHIKI_THEME_LIGHT,
  getShikiLangLoader,
  isPlainShikiLang,
  toShikiLangId,
} from './shikiLanguages';

export type ShikiTokenSpan = {
  content: string;
  style: Record<string, string>;
};

export type ShikiLine = ShikiTokenSpan[];

type Highlighter = Awaited<ReturnType<typeof createHighlighterCore>>;
type GrammarState = NonNullable<ReturnType<Highlighter['codeToTokens']>['grammarState']>;

type HighlightFn = (
  code: string,
  lang: string,
  grammarState?: GrammarState
) => { lines: ShikiLine[]; grammarState?: GrammarState };

const DUAL_THEME_OPTIONS = {
  themes: {
    light: SHIKI_THEME_LIGHT,
    dark: SHIKI_THEME_DARK,
  },
  defaultColor: false as const,
};

/** Common fences: warm their grammars on idle so the first paint is Shiki, not hljs. */
const PREFETCH_SHIKI_LANGUAGES = ['typescript', 'javascript', 'python', 'bash', 'json', 'tsx'] as const;

let highlighterPromise: Promise<Highlighter> | null = null;
let highlighter: Highlighter | null = null;
const loadedLangs = new Set<string>();
const failedLangs = new Set<string>();
const inflightLangs = new Map<string, Promise<void>>();

const getHighlighterPromise = (): Promise<Highlighter> => {
  highlighterPromise ??= createHighlighterCore({
    themes: [githubLightDefault, githubDarkDefault],
    langs: [],
    engine: createJavaScriptRegexEngine(),
  }).then((instance) => {
    highlighter = instance;
    return instance;
  });
  return highlighterPromise;
};

void getHighlighterPromise();

const emptyLine = (): ShikiLine => [{ content: '', style: {} }];

const toPlainLines = (code: string): ShikiLine[] =>
  code.split('\n').map((line) => [{ content: line, style: {} }]);

const alignLineCount = (lines: ShikiLine[], count: number): ShikiLine[] => {
  if (lines.length === count) return lines;
  if (lines.length > count) return lines.slice(0, count);
  const next = lines.slice();
  while (next.length < count) next.push(emptyLine());
  return next;
};

const tokensToLines = (tokens: ReturnType<Highlighter['codeToTokens']>['tokens']): ShikiLine[] => {
  if (tokens.length === 0) return [[{ content: '', style: {} }]];
  return tokens.map((line) => {
    if (line.length === 0) return [{ content: '', style: {} }];
    return line.map((token) => ({
      content: token.content,
      style: token.htmlStyle ? { ...token.htmlStyle } : {},
    }));
  });
};

const highlightWith = (
  instance: Highlighter,
  code: string,
  lang: string,
  grammarState?: GrammarState
): { lines: ShikiLine[]; grammarState?: GrammarState } => {
  if (isPlainShikiLang(lang) || failedLangs.has(lang)) {
    return { lines: toPlainLines(code) };
  }
  try {
    const result = instance.codeToTokens(code, {
      lang: toShikiLangId(lang),
      ...DUAL_THEME_OPTIONS,
      grammarState,
    });
    return { lines: tokensToLines(result.tokens), grammarState: result.grammarState };
  } catch {
    failedLangs.add(lang);
    return { lines: toPlainLines(code) };
  }
};

export const isShikiLanguageReady = (resolved: string): boolean => {
  if (isPlainShikiLang(resolved)) return true;
  if (!highlighter) return false;
  return loadedLangs.has(resolved) || failedLangs.has(resolved);
};

export const ensureShikiLanguage = async (resolved: string): Promise<void> => {
  await getHighlighterPromise();
  if (isPlainShikiLang(resolved) || loadedLangs.has(resolved) || failedLangs.has(resolved)) return;

  const pending = inflightLangs.get(resolved);
  if (pending) {
    await pending;
    return;
  }

  const loader = getShikiLangLoader(resolved);
  if (!loader) {
    failedLangs.add(resolved);
    return;
  }

  const task = (async () => {
    try {
      const instance = await getHighlighterPromise();
      await instance.loadLanguage(loader);
      try {
        // The JS regex engine can emit collapsed scopes on the first
        // codeToTokens after loadLanguage. Prime once before serving.
        instance.codeToTokens('//\n', {
          lang: toShikiLangId(resolved),
          ...DUAL_THEME_OPTIONS,
        });
      } catch {
        /* priming is best-effort */
      }
      loadedLangs.add(resolved);
    } catch {
      failedLangs.add(resolved);
    } finally {
      inflightLangs.delete(resolved);
    }
  })();

  inflightLangs.set(resolved, task);
  await task;
};

const scheduleCommonLanguagePrefetch = (): void => {
  if (typeof window === 'undefined') return;
  const prefetch = () => {
    for (const lang of PREFETCH_SHIKI_LANGUAGES) {
      void ensureShikiLanguage(lang);
    }
  };
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(prefetch, { timeout: 2500 });
    return;
  }
  window.setTimeout(prefetch, 1);
};

scheduleCommonLanguagePrefetch();

export const tokenizeCode = (code: string, resolved: string): ShikiLine[] | null => {
  if (isPlainShikiLang(resolved)) return toPlainLines(code);
  if (!highlighter || !isShikiLanguageReady(resolved)) return null;
  return highlightWith(highlighter, code, resolved).lines;
};

export class StreamingHighlightCache {
  private lang = '';
  private prefixCount = 0;
  private prefix = '';
  private prefixLines: ShikiLine[] = [];
  private grammarState: GrammarState | undefined;
  private lastLine = '';
  private lastLines: ShikiLine[] = [];
  private lastGrammarState: GrammarState | undefined;

  reset(): void {
    this.lang = '';
    this.prefixCount = 0;
    this.prefix = '';
    this.prefixLines = [];
    this.grammarState = undefined;
    this.lastLine = '';
    this.lastLines = [];
    this.lastGrammarState = undefined;
  }

  private assembledCode(): string {
    if (this.prefixCount === 0) return this.lastLine;
    return `${this.prefix}\n${this.lastLine}`;
  }

  snapshotIfMatches(code: string, resolved: string): ShikiLine[] | null {
    if (this.lang !== resolved) return null;
    if (this.assembledCode() !== code) return null;
    if (this.prefixLines.length === 0 && this.lastLines.length === 0) return null;
    return [...this.prefixLines, ...this.lastLines];
  }

  apply(code: string, resolved: string, streaming: boolean, highlight: HighlightFn): ShikiLine[] {
    if (!streaming) {
      const reused = this.snapshotIfMatches(code, resolved);
      if (reused) return reused;
      this.reset();
      return highlight(code, resolved).lines;
    }

    if (this.lang !== resolved) this.reset();
    this.lang = resolved;

    const parts = code.split('\n');
    const prefixCount = Math.max(0, parts.length - 1);
    const prefixParts = parts.slice(0, prefixCount);
    const prefix = prefixParts.join('\n');
    const lastLine = parts[prefixCount] ?? '';

    if (prefixCount === this.prefixCount && prefix === this.prefix) {
      if (lastLine !== this.lastLine || this.lastLines.length === 0) {
        const result = highlight(lastLine, resolved, this.grammarState);
        this.lastLine = lastLine;
        this.lastLines = result.lines;
        this.lastGrammarState = result.grammarState;
      }
      return [...this.prefixLines, ...this.lastLines];
    }

    const completed = this.prefixCount === 0 ? this.lastLine : `${this.prefix}\n${this.lastLine}`;
    if (prefixCount === this.prefixCount + 1 && prefix === completed) {
      this.prefixCount = prefixCount;
      this.prefix = prefix;
      this.prefixLines = [...this.prefixLines, ...this.lastLines];
      this.grammarState = this.lastGrammarState;
      const result = highlight(lastLine, resolved, this.grammarState);
      this.lastLine = lastLine;
      this.lastLines = result.lines;
      this.lastGrammarState = result.grammarState;
      return [...this.prefixLines, ...this.lastLines];
    }

    if (prefixCount > 0) {
      const result = highlight(prefix, resolved);
      this.prefixCount = prefixCount;
      this.prefix = prefix;
      this.prefixLines = alignLineCount(result.lines, prefixCount);
      this.grammarState = result.grammarState;
    } else {
      this.prefixCount = 0;
      this.prefix = '';
      this.prefixLines = [];
      this.grammarState = undefined;
    }
    const last = highlight(lastLine, resolved, this.grammarState);
    this.lastLine = lastLine;
    this.lastLines = last.lines;
    this.lastGrammarState = last.grammarState;
    return [...this.prefixLines, ...this.lastLines];
  }
}

export const tokenizeCodeStreaming = (
  code: string,
  resolved: string,
  streaming: boolean,
  cache: StreamingHighlightCache
): ShikiLine[] | null => {
  if (isPlainShikiLang(resolved)) {
    return cache.apply(code, resolved, streaming, (snippet) => ({ lines: toPlainLines(snippet) }));
  }
  if (!highlighter || !isShikiLanguageReady(resolved)) return null;
  const instance = highlighter;
  return cache.apply(code, resolved, streaming, (snippet, lang, grammarState) =>
    highlightWith(instance, snippet, lang, grammarState)
  );
};
