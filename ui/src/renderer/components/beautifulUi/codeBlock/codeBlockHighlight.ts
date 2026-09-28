import type { CSSProperties } from 'react';

// Token colours resolve through CSS custom properties on :root /
// [data-theme='dark'] (flowy-visual-system.css) so light/dark themes switch
// without re-rendering. Fallbacks stay readable if the stylesheet is absent.
const token = (name: string, fallback: string) => `var(--code-token-${name}, ${fallback})`;

const text = token('text', '#1f2328');
const keyword = token('keyword', '#cf222e');
const fn = token('function', '#8250df');
const type = token('type', '#953800');
const constant = token('constant', '#0550ae');
const str = token('string', '#0a3069');
const comment = token('comment', '#6e7781');
const tag = token('tag', '#116329');
const variable = token('variable', '#953800');
const punctuation = token('punctuation', '#57606a');

/** Highlight.js token colours for Beautiful UI Code Block (GitHub-style palette). */
export const beautifulUiHighlightStyle: { [key: string]: CSSProperties } = {
  hljs: {
    background: 'transparent',
    color: text,
  },
  'hljs-keyword': { color: keyword },
  'hljs-meta-keyword': { color: keyword },
  'hljs-selector-tag': { color: keyword },
  'hljs-template-tag': { color: keyword },
  'hljs-built_in': { color: type },
  'hljs-type': { color: type },
  'hljs-class': { color: type },
  'hljs-title': { color: fn },
  'hljs-title.class_': { color: type },
  'hljs-title.class_.inherited__': { color: type },
  'hljs-title.function_': { color: fn },
  'hljs-section': { color: fn, fontWeight: 600 },
  'hljs-literal': { color: constant },
  'hljs-number': { color: constant },
  'hljs-symbol': { color: constant },
  'hljs-attr': { color: constant },
  'hljs-attribute': { color: constant },
  'hljs-selector-id': { color: constant },
  'hljs-selector-class': { color: constant },
  'hljs-selector-attr': { color: constant },
  'hljs-selector-pseudo': { color: constant },
  'hljs-property': { color: constant },
  'hljs-string': { color: str },
  'hljs-regexp': { color: str },
  'hljs-link': { color: str, textDecoration: 'underline' },
  'hljs-subst': { color: text },
  'hljs-template-variable': { color: variable },
  'hljs-variable': { color: variable },
  'hljs-variable.language_': { color: keyword },
  'hljs-params': { color: text },
  'hljs-comment': { color: comment, fontStyle: 'italic' },
  'hljs-doctag': { color: keyword },
  'hljs-quote': { color: tag },
  'hljs-tag': { color: punctuation },
  'hljs-name': { color: tag },
  'hljs-bullet': { color: variable },
  'hljs-meta': { color: constant },
  'hljs-punctuation': { color: punctuation },
  'hljs-operator': { color: keyword },
  'hljs-emphasis': { fontStyle: 'italic' },
  'hljs-strong': { fontWeight: 600 },
  'hljs-addition': { color: tag },
  'hljs-deletion': { color: 'var(--color-danger-6, #f53f3f)' },
};
