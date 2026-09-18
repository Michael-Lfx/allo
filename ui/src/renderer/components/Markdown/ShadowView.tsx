

import { theme } from '@/platform';
import React, { useState } from 'react';
import ReactDOM from 'react-dom';
import { addImportantToAll } from '@renderer/utils/theme/customCssProcessor';
import { configService } from '@/common/config/configService';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';

/**
 * Create the base style element for Shadow DOM with CSS variables, theme styles, and optional custom CSS.
 */
const createInitStyle = (
  currentTheme = 'light',
  cssVars?: Record<string, string>,
  customCss?: string,
  isMobile?: boolean,
  fontSize?: string,
  lineHeight?: string
) => {
  const style = document.createElement('style');
  // Inject external CSS variables into Shadow DOM for dark mode support
  const cssVarsDeclaration = cssVars
    ? Object.entries(cssVars)
        .map(([key, value]) => `${key}: ${value};`)
        .join('\n    ')
    : '';

  const resolvedFontSize = fontSize ?? (isMobile ? '14px' : '16px');
  const resolvedLineHeight = lineHeight ?? (isMobile ? '19.6px' : '28px');
  const usesExplicitTypography = Boolean(fontSize || lineHeight);

  style.innerHTML = `
  /* Shadow DOM CSS variable definitions */
  :host {
    ${cssVarsDeclaration}
  }

  * {
    line-height:${resolvedLineHeight};
    font-size:${resolvedFontSize};
    color: inherit;
  }

  .markdown-shadow-body {
    word-break: break-word;
    overflow-wrap: anywhere;
    color: var(--text-primary);
    max-width: 100%;
  }
  .markdown-shadow-body>p:first-child,
  .markdown-shadow-body>h1:first-child,
  .markdown-shadow-body>h2:first-child,
  .markdown-shadow-body>h3:first-child
  {
    margin-top:0px;
  }
  h1,h2,h3,h4,h5,h6{
    margin-block-start:0px;
    margin-block-end:0px;
  }
  .markdown-shadow-body p {
    margin-block-start: ${usesExplicitTypography ? '10px' : '16px'};
    margin-block-end: ${usesExplicitTypography ? '10px' : '16px'};
  }
  .markdown-shadow-body li {
    margin-block-start: ${usesExplicitTypography ? '4px' : '6px'};
    margin-block-end: ${usesExplicitTypography ? '4px' : '6px'};
  }
  a{
    color: var(--color-link, ${theme.Color.PrimaryColor});
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
    word-break: break-all;
    overflow-wrap: anywhere;
  }
  a:hover {
    color: var(--color-link-hover, ${theme.Color.PrimaryColor});
  }
  a:focus-visible {
    border-radius: 2px;
    outline: 2px solid var(--color-link-hover, ${theme.Color.PrimaryColor});
    outline-offset: 2px;
  }
  h1{
    font-size: ${usesExplicitTypography ? '18px' : '24px'};
    line-height: ${usesExplicitTypography ? '26px' : '32px'};
    font-weight: bold;
    margin-top: ${usesExplicitTypography ? '10px' : '20px'};
    margin-bottom: ${usesExplicitTypography ? '6px' : '12px'};
  }
  h2{
    font-size: ${usesExplicitTypography ? '16px' : '18px'};
    line-height: ${usesExplicitTypography ? '24px' : '26px'};
    font-weight: 600;
    margin-top: ${usesExplicitTypography ? '10px' : '20px'};
    margin-bottom: ${usesExplicitTypography ? '6px' : '12px'};
  }
  h3{
    font-size: ${usesExplicitTypography ? '15px' : '16px'};
    line-height: ${usesExplicitTypography ? '22px' : '24px'};
    font-weight: 600;
    margin-top: ${usesExplicitTypography ? '8px' : '16px'};
    margin-bottom: ${usesExplicitTypography ? '4px' : '10px'};
  }
  h4,h5,h6{
    font-size: ${usesExplicitTypography ? '14px' : '15px'};
    line-height: 22px;
    font-weight: 600;
    margin-top: ${usesExplicitTypography ? '8px' : '14px'};
    margin-bottom: ${usesExplicitTypography ? '6px' : '8px'};
  }
  code span{
    font-size:13px;
    line-height:20px;
  }

  .markdown-shadow-body>p:last-child{
    margin-bottom:0px;
  }
  .markdown-shadow-body > :last-child::after {
    content: var(--beautiful-ui-streaming-caret-content, none);
    display: inline-block;
    width: 2px;
    height: 1em;
    margin-left: 2px;
    margin-right: -4px;
    vertical-align: -0.12em;
    background: var(--beautiful-ui-streaming-caret-bg, var(--color-text-1, #1d2129));
    box-shadow: var(--beautiful-ui-streaming-caret-shadow, none);
    animation: var(--beautiful-ui-streaming-caret-animation, none);
  }
  @keyframes streaming-caret {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0;
    }
  }
  ol, ul {
    padding-inline-start:24px;
  }
  hr {
    border: none;
    border-top: 1px solid var(--bg-3);
    margin: 28px 0;
  }
  strong {
    font-weight: 600;
    color: var(--text-primary);
  }
  .markdown-shadow-body code:not(pre code) {
    background: none;
    color: inherit;
    padding: 0;
    border: 0;
    border-radius: 0;
    font-size: 0.94em;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }
  blockquote {
    border-left: 3px solid var(--bg-3);
    padding-left: 12px;
    color: var(--text-primary);
    margin: 16px 0;
  }
  /* CollapsibleContent is shared with light-DOM surfaces, so keep its
     structural styles here as well when Markdown renders inside Shadow DOM. */
  .collapsible-content {
    position: relative;
    min-width: 0;
  }
  .collapsible-content__body {
    min-width: 0;
    transition-property: max-height, opacity;
    transition-duration: 300ms;
    transition-timing-function: ease;
  }
  .collapsible-content__mask {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    pointer-events: none;
  }
  .collapsible-content__controls {
    position: relative;
    z-index: 1;
    display: flex;
    justify-content: center;
  }
  .collapsible-content__toggle {
    display: inline-flex;
    min-height: 30px;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 6px 12px;
    font-size: 13px;
    font-weight: 500;
    line-height: 18px;
    transition: color 160ms ease, background-color 160ms ease;
  }
  .collapsible-content__toggle:hover {
    background: var(--bg-3);
    color: var(--text-primary);
  }
  .collapsible-content__toggle:active {
    background: var(--bg-1);
  }
  .collapsible-content__toggle:focus-visible {
    outline: 2px solid var(--color-link, ${theme.Color.PrimaryColor});
    outline-offset: 1px;
  }
  .collapsible-content__toggle svg {
    display: inline-block;
    flex: none;
    vertical-align: middle;
  }
  pre {
    max-width: 100%;
    // overflow-x: auto;
    margin-block-start: 8px;
    margin-block-end: 8px;
  }
  /* Code block horizontal scrollbar — blends with bg-2 */
  pre,
  .hljs,
  .markdown-code-content {
    scrollbar-width: thin;
    scrollbar-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.18)' : 'rgba(0, 0, 0, 0.1)'} transparent;
  }
  pre::-webkit-scrollbar,
  .hljs::-webkit-scrollbar,
  .markdown-code-content::-webkit-scrollbar {
    height: 6px;
    background: transparent;
  }
  pre::-webkit-scrollbar-track,
  .hljs::-webkit-scrollbar-track,
  .markdown-code-content::-webkit-scrollbar-track,
  pre::-webkit-scrollbar-corner,
  .hljs::-webkit-scrollbar-corner,
  .markdown-code-content::-webkit-scrollbar-corner {
    background: transparent;
  }
  pre::-webkit-scrollbar-thumb,
  .hljs::-webkit-scrollbar-thumb,
  .markdown-code-content::-webkit-scrollbar-thumb {
    background-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.18)' : 'rgba(0, 0, 0, 0.1)'};
    border-radius: 3px;
  }
  pre::-webkit-scrollbar-thumb:hover,
  .hljs::-webkit-scrollbar-thumb:hover,
  .markdown-code-content::-webkit-scrollbar-thumb:hover {
    background-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.28)' : 'rgba(0, 0, 0, 0.2)'};
  }
  .markdown-code-block {
    min-width: 0;
    max-width: 100%;
  }
  .markdown-code-surface {
    overflow: hidden;
    border: 1px solid var(--bg-3);
    border-radius: 8px;
    background: var(--bg-2);
  }
  .markdown-code-header {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
  }
  .markdown-code-language,
  .markdown-code-footer-label {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
  }
  .markdown-code-content {
    min-width: 0;
  }
  .markdown-code-toolbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    color: var(--text-secondary);
  }
  .markdown-code-action {
    display: inline-flex;
    min-width: 30px;
    min-height: 30px;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    padding: 0;
  }
  .markdown-code-action:hover {
    background: var(--bg-3);
    color: var(--text-primary);
  }
  .markdown-code-action:active {
    background: var(--bg-1);
  }
  .markdown-code-action:focus-visible {
    outline: 2px solid var(--color-link, ${theme.Color.PrimaryColor});
    outline-offset: 1px;
  }
  .markdown-code-footer {
    display: flex;
    width: 100%;
    min-height: 30px;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border: 0;
    border-top: 1px solid var(--bg-3);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 6px 12px;
  }
  .markdown-code-footer:hover {
    background: var(--bg-3);
    color: var(--text-primary);
  }
  .markdown-code-footer:active {
    background: var(--bg-1);
  }
  .markdown-code-footer:focus-visible {
    outline: 2px solid var(--color-link, ${theme.Color.PrimaryColor});
    outline-offset: -2px;
  }
  .markdown-mermaid-block {
    width: 100%;
    min-width: 0;
    max-width: 100%;
  }
  .markdown-mermaid-surface {
    overflow: hidden;
    overflow-x: auto;
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    background: var(--bg-1);
  }
  .markdown-mermaid-header {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    border-bottom: 1px solid var(--bg-3);
    background: var(--bg-2);
    padding: 6px 10px;
  }
  .markdown-mermaid-leading,
  .markdown-mermaid-toolbar,
  .markdown-mermaid-segmented {
    display: flex;
    min-width: 0;
    align-items: center;
  }
  .markdown-mermaid-leading {
    gap: 8px;
  }
  .markdown-mermaid-toolbar {
    flex: none;
    color: var(--text-secondary);
  }
  .markdown-mermaid-language {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 20px;
  }
  .markdown-mermaid-segmented {
    gap: 2px;
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    background: var(--bg-1);
    padding: 2px;
  }
  .markdown-mermaid-segment {
    min-height: 24px;
    border: 0;
    border-radius: 3px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    line-height: 20px;
    padding: 1px 6px;
  }
  .markdown-mermaid-segment:hover {
    background: var(--bg-3);
    color: var(--text-primary);
  }
  .markdown-mermaid-segment:active {
    background: var(--bg-2);
  }
  .markdown-mermaid-segment[aria-pressed='true'] {
    background: var(--bg-3);
    color: var(--text-primary);
    font-weight: 600;
  }
  .markdown-mermaid-segment:focus-visible {
    outline: 2px solid var(--color-link, ${theme.Color.PrimaryColor});
    outline-offset: 1px;
  }
  .markdown-mermaid-diagram {
    display: flex;
    justify-content: center;
    overflow-x: auto;
    background: var(--bg-1);
    padding: 12px;
  }
  .markdown-mermaid-loading {
    display: flex;
    min-height: 52px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    background: var(--bg-1);
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 20px;
    padding: 16px 12px;
  }
  .markdown-mermaid-loading-indicator {
    flex: none;
    width: 12px;
    height: 12px;
    border: 2px solid var(--bg-3);
    border-radius: 999px;
    border-top-color: var(--text-secondary);
  }
  .markdown-figure-block {
    width: 100%;
    min-width: 0;
    max-width: 100%;
  }
  .markdown-figure-surface {
    overflow: hidden;
    overflow-x: auto;
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    background: var(--bg-1);
  }
  .markdown-figure-header {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    border-bottom: 1px solid var(--bg-3);
    background: var(--bg-2);
    padding: 6px 10px;
  }
  .markdown-figure-leading,
  .markdown-figure-toolbar,
  .markdown-figure-segmented {
    display: flex;
    min-width: 0;
    align-items: center;
  }
  .markdown-figure-leading {
    gap: 8px;
  }
  .markdown-figure-toolbar {
    flex: none;
    color: var(--text-secondary);
  }
  .markdown-figure-language {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 20px;
  }
  .markdown-figure-segmented {
    gap: 2px;
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    background: var(--bg-1);
    padding: 2px;
  }
  .markdown-figure-segment {
    min-height: 24px;
    border: 0;
    border-radius: 3px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    line-height: 20px;
    padding: 1px 6px;
  }
  .markdown-figure-segment:hover {
    background: var(--bg-3);
    color: var(--text-primary);
  }
  .markdown-figure-segment:active {
    background: var(--bg-2);
  }
  .markdown-figure-segment[aria-pressed='true'] {
    background: var(--bg-3);
    color: var(--text-primary);
    font-weight: 600;
  }
  .markdown-figure-segment:focus-visible {
    outline: 2px solid var(--color-link, ${theme.Color.PrimaryColor});
    outline-offset: 1px;
  }
  .markdown-figure-svg {
    display: flex;
    justify-content: center;
    overflow-x: auto;
    background: var(--bg-1);
    color: var(--text-primary);
    padding: 12px;
  }
  .markdown-figure-svg svg {
    max-width: 100%;
    height: auto;
    display: block;
  }
  .markdown-figure-jsxgraph {
    /* Minimal stand-in for jsxgraph.css's .jxgbox: the package's "exports"
       map does not expose the stylesheet, so the rules the board needs
       (relative, clipped, no touch scrolling) are owned here. */
    position: relative;
    overflow: hidden;
    touch-action: none;
    width: 100%;
    height: 340px;
    background: var(--bg-1);
  }
  .markdown-figure-source {
    background: var(--bg-1);
  }
  .markdown-figure-error {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    border-bottom: 1px solid var(--bg-3);
    color: var(--color-danger, #f53f3f);
    font-size: 12px;
    line-height: 20px;
    padding: 6px 10px;
  }
  .markdown-figure-error-text {
    flex: 1;
    min-width: 0;
    word-break: break-word;
  }
  .markdown-figure-error .markdown-figure-segment {
    flex: none;
  }
  @media (hover: hover) and (pointer: fine) {
    .markdown-code-toolbar {
      opacity: 0;
      pointer-events: none;
      transition: opacity 0.16s ease;
    }
    .markdown-code-block:hover .markdown-code-toolbar,
    .markdown-code-block:focus-within .markdown-code-toolbar,
    .markdown-mermaid-block:hover .markdown-mermaid-toolbar,
    .markdown-mermaid-block:focus-within .markdown-mermaid-toolbar,
    .markdown-figure-block:hover .markdown-figure-toolbar,
    .markdown-figure-block:focus-within .markdown-figure-toolbar {
      opacity: 1;
      pointer-events: auto;
    }
  }
  img {
    max-width: 100%;
    height: auto;
  }
  .markdown-shadow-body > .markdown-table-wrap:first-child {
    margin-top: 0;
  }
  .markdown-table-wrap {
    max-width: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    margin: 12px 0;
    border: 1px solid var(--color-border-2, var(--bg-3));
    border-radius: 12px;
    background: var(--color-bg-1, var(--bg-1));
    scrollbar-width: thin;
    scrollbar-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.18)' : 'rgba(0, 0, 0, 0.1)'} transparent;
  }
  .markdown-table-wrap::-webkit-scrollbar {
    height: 6px;
    background: transparent;
  }
  .markdown-table-wrap::-webkit-scrollbar-track,
  .markdown-table-wrap::-webkit-scrollbar-corner {
    background: transparent;
  }
  .markdown-table-wrap::-webkit-scrollbar-thumb {
    background-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.18)' : 'rgba(0, 0, 0, 0.1)'};
    border-radius: 3px;
  }
  .markdown-table-wrap::-webkit-scrollbar-thumb:hover {
    background-color: ${currentTheme === 'dark' ? 'rgba(255, 255, 255, 0.28)' : 'rgba(0, 0, 0, 0.2)'};
  }
  .markdown-table-wrap table {
    width: 100%;
    min-width: 100%;
    border-collapse: separate;
    border-spacing: 0;
  }
  .markdown-table-wrap th,
  .markdown-table-wrap td {
    padding: 8px 12px;
    border: 0;
    border-bottom: 1px solid var(--color-border-2, var(--bg-3));
    vertical-align: top;
  }
  .markdown-table-wrap th {
    background: var(--color-fill-1, var(--bg-1));
    color: var(--color-text-3, var(--text-secondary));
    font-size: 0.85em;
    font-weight: 600;
    line-height: 1.4;
    white-space: nowrap;
  }
  .markdown-table-wrap td {
    color: var(--color-text-1, var(--text-primary));
    overflow-wrap: anywhere;
  }
  .markdown-table-wrap tbody tr:last-child td {
    border-bottom: 0;
  }
  .markdown-table-wrap th:first-child,
  .markdown-table-wrap td:first-child {
    position: sticky;
    left: 0;
    z-index: 1;
    font-weight: 600;
    background: var(--color-bg-1, var(--bg-1));
    box-shadow: 1px 0 0 var(--color-border-2, var(--bg-3));
  }
  .markdown-table-wrap thead th:first-child {
    z-index: 2;
    background: var(--color-fill-1, var(--bg-1));
  }
  @media (hover: hover) and (pointer: fine) {
    .markdown-table-wrap tbody tr:hover td,
    .markdown-table-wrap tbody tr:hover td:first-child {
      background: var(--color-fill-1, var(--bg-2));
      transition: background 0.12s ease;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .collapsible-content__body,
    .collapsible-content__toggle {
      transition: none;
    }
    .markdown-table-wrap tbody tr:hover td {
      transition: none;
    }
  }
  /* Inline code should wrap on small screens to avoid horizontal overflow */
  .markdown-shadow-body code {
    word-break: break-word;
    overflow-wrap: anywhere;
    max-width: 100%;
  }
  .markdown-shadow-body .hljs,
  .markdown-shadow-body .hljs code {
    max-width: 100%;
    overflow: visible;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    word-break: break-word;
  }
  /* Allow KaTeX to use its own line-height for proper fraction/superscript rendering */
  .katex,
  .katex * {
    line-height: normal;
  }

  /* Display math: only scroll horizontally when formula exceeds container width */
  .katex-display {
    overflow-x: auto;
    overflow-y: hidden;
    padding: 0.5em 0;
  }

  .loading {
    animation: loading 1s linear infinite;
  }


  @keyframes loading {
    0% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loading {
      animation: none;
    }
    .markdown-shadow-body > :last-child::after {
      animation: none;
      background: var(--color-text-1, #1d2129);
      box-shadow: none;
    }
  }

  /* User Custom CSS (injected into Shadow DOM) */
  ${customCss || ''}
  `;
  return style;
};

// Cache for KaTeX stylesheet to share across Shadow DOM instances
let katexStyleSheet: CSSStyleSheet | null = null;

/**
 * Get or create a shared KaTeX CSSStyleSheet for Shadow DOM adoption.
 * This extracts KaTeX styles from the document and creates a constructable stylesheet.
 */
const getKatexStyleSheet = (): CSSStyleSheet | null => {
  if (katexStyleSheet) return katexStyleSheet;

  try {
    // Find the KaTeX stylesheet in the document
    const katexSheet = [...document.styleSheets].find(
      (sheet) => sheet.href?.includes('katex') || (sheet.ownerNode as HTMLElement)?.dataset?.katex
    );

    if (katexSheet) {
      const cssRules = [...katexSheet.cssRules].map((rule) => rule.cssText).join('\n');
      katexStyleSheet = new CSSStyleSheet();
      katexStyleSheet.replaceSync(cssRules);
      return katexStyleSheet;
    }

    // Fallback: try to find KaTeX styles by checking style tags
    const styleSheets = [...document.styleSheets];
    for (const sheet of styleSheets) {
      try {
        const rules = [...sheet.cssRules];
        // Check if this stylesheet contains KaTeX rules
        const hasKatexRules = rules.some((rule) => rule.cssText.includes('.katex'));
        if (hasKatexRules) {
          const cssRules = rules.map((rule) => rule.cssText).join('\n');
          katexStyleSheet = new CSSStyleSheet();
          katexStyleSheet.replaceSync(cssRules);
          return katexStyleSheet;
        }
      } catch {
        // CORS may block access to cssRules for external stylesheets
        continue;
      }
    }
  } catch (error) {
    console.warn('Failed to create KaTeX stylesheet for Shadow DOM:', error);
  }

  return null;
};

type ShadowDivElement = HTMLDivElement & { __init__shadow?: boolean };

const ShadowView = ({
  children,
  fontSize,
  lineHeight,
}: {
  children: React.ReactNode;
  fontSize?: string;
  lineHeight?: string;
}) => {
  const [root, setRoot] = useState<ShadowRoot | null>(null);
  const styleRef = React.useRef<HTMLStyleElement | null>(null);
  const [customCss, setCustomCss] = useState<string>('');
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;

  React.useEffect(() => {
    const css = configService.get('customCss');
    if (css) {
      setCustomCss(addImportantToAll(css));
    } else {
      setCustomCss('');
    }

    // Listen to custom CSS update events
    const handleCustomCssUpdate = (e: CustomEvent) => {
      if (e.detail?.customCss !== undefined) {
        const css = e.detail.customCss || '';
        // Use unified utility to auto-add !important
        const processedCss = addImportantToAll(css);
        setCustomCss(processedCss);
      }
    };

    window.addEventListener('custom-css-updated', handleCustomCssUpdate as EventListener);

    return () => {
      window.removeEventListener('custom-css-updated', handleCustomCssUpdate as EventListener);
    };
  }, []);

  // Update CSS variables and custom styles in Shadow DOM
  const updateStyles = React.useCallback(
    (shadowRoot: ShadowRoot) => {
      const computedStyle = getComputedStyle(document.documentElement);
      const currentTheme = document.documentElement.getAttribute('data-theme') || 'light';
      const cssVars = {
        '--bg-1': computedStyle.getPropertyValue('--bg-1'),
        '--bg-2': computedStyle.getPropertyValue('--bg-2'),
        '--bg-3': computedStyle.getPropertyValue('--bg-3'),
        '--color-bg-1': computedStyle.getPropertyValue('--color-bg-1'),
        '--color-border-2': computedStyle.getPropertyValue('--color-border-2'),
        '--color-fill-1': computedStyle.getPropertyValue('--color-fill-1'),
        '--color-text-1': computedStyle.getPropertyValue('--color-text-1'),
        '--color-text-2': computedStyle.getPropertyValue('--color-text-2'),
        '--color-text-3': computedStyle.getPropertyValue('--color-text-3'),
        '--text-primary': computedStyle.getPropertyValue('--text-primary'),
        '--text-secondary': computedStyle.getPropertyValue('--text-secondary'),
        '--color-link':
          computedStyle.getPropertyValue('--color-primary-6').trim() || theme.Color.PrimaryColor,
        '--color-link-hover':
          computedStyle.getPropertyValue('--color-primary-7').trim() || theme.Color.PrimaryColor,
      };

      // Remove old style and add new style
      if (styleRef.current) {
        styleRef.current.remove();
      }
      const newStyle = createInitStyle(currentTheme, cssVars, customCss, isMobile, fontSize, lineHeight);
      styleRef.current = newStyle;
      shadowRoot.appendChild(newStyle);

      // Inject KaTeX styles into Shadow DOM using adoptedStyleSheets
      // This allows math expressions to render correctly
      const katexSheet = getKatexStyleSheet();
      if (katexSheet && !shadowRoot.adoptedStyleSheets.includes(katexSheet)) {
        shadowRoot.adoptedStyleSheets = [...shadowRoot.adoptedStyleSheets, katexSheet];
      }
    },
    [customCss, fontSize, isMobile, lineHeight]
  );

  React.useEffect(() => {
    if (!root) return;

    // Update styles when custom CSS changes
    updateStyles(root);
  }, [root, customCss, updateStyles]);

  React.useEffect(() => {
    if (!root) return;

    // Listen for theme changes
    const observer = new MutationObserver(() => {
      updateStyles(root);
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'class'],
    });

    return () => observer.disconnect();
  }, [root, updateStyles]);

  return (
    <div
      ref={(el: ShadowDivElement | null) => {
        if (!el || el.__init__shadow) return;
        el.__init__shadow = true;
        const shadowRoot = el.attachShadow({ mode: 'open' });
        updateStyles(shadowRoot);
        setRoot(shadowRoot);
      }}
      className='markdown-shadow'
      style={{ width: '100%', flex: '1 1 auto', minWidth: 0 }}
    >
      {root && ReactDOM.createPortal(children, root)}
    </div>
  );
};

export default ShadowView;
