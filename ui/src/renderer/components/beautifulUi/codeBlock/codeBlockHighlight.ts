import type { CSSProperties } from 'react';

const ink = 'var(--color-text-1, #1d2129)';
const muted = 'var(--color-text-2, #4e5969)';
const dim = 'var(--color-text-3, #86909c)';
const str = 'var(--color-success-6, #00b42a)';
const num = 'var(--color-warning-6, #ff7d00)';

/** Highlight.js token colors aligned with Beautiful UI Code Block, using Flowy theme tokens. */
export const beautifulUiHighlightStyle: { [key: string]: CSSProperties } = {
  hljs: {
    background: 'transparent',
    color: muted,
  },
  'hljs-keyword': { color: ink },
  'hljs-built_in': { color: ink },
  'hljs-type': { color: ink },
  'hljs-literal': { color: ink },
  'hljs-symbol': { color: ink },
  'hljs-title': { color: ink },
  'hljs-title.class_': { color: ink },
  'hljs-title.function_': { color: ink },
  'hljs-function': { color: ink },
  'hljs-params': { color: muted },
  'hljs-string': { color: str },
  'hljs-attr': { color: str },
  'hljs-attribute': { color: str },
  'hljs-number': { color: num },
  'hljs-number.': { color: num },
  'hljs-comment': { color: dim },
  'hljs-doctag': { color: dim },
  'hljs-punctuation': { color: dim },
  'hljs-operator': { color: dim },
  'hljs-tag': { color: muted },
  'hljs-name': { color: ink },
  'hljs-variable': { color: muted },
  'hljs-property': { color: muted },
  'hljs-meta': { color: dim },
  'hljs-addition': { color: str },
  'hljs-deletion': { color: 'var(--color-danger-6, #f53f3f)' },
};
