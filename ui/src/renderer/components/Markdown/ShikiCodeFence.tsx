/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { adoptBeautifulUiCodeBlockCss } from '@renderer/components/beautifulUi/codeBlock/adoptCodeBlockCss';
import styles from '@renderer/components/beautifulUi/codeBlock/codeBlock.module.css';
import { getDiffLineStyle } from './markdownUtils';
import HljsCodeFence from './HljsCodeFence';
import {
  StreamingHighlightCache,
  ensureShikiLanguage,
  isShikiLanguageReady,
  tokenizeCode,
  tokenizeCodeStreaming,
  type ShikiLine,
} from './shikiHighlighter';
import SyntaxHighlightBoundary from './SyntaxHighlightBoundary';

const EMPTY_DIFF_LINES: string[] = [];

export type ShikiCodeFenceProps = {
  content: string;
  language: string;
  isStreaming?: boolean;
  isDiff?: boolean;
  isDark?: boolean;
  diffLines?: string[];
  showLineNumbers?: boolean;
};

const ShikiLines = React.memo(function ShikiLines({
  lines,
  isDiff,
  isDark,
  diffLines,
  showLineNumbers,
}: {
  lines: ShikiLine[];
  isDiff: boolean;
  isDark: boolean;
  diffLines: string[];
  showLineNumbers: boolean;
}) {
  return (
    <div className={`shiki ${styles.shikiRoot}`} data-highlighter='shiki'>
      {lines.map((line, index) => {
        const isEmpty = line.every((token) => token.content.length === 0);
        return (
          <div
            key={index}
            className={styles.line}
            style={isDiff ? getDiffLineStyle(diffLines[index] || '', isDark) : undefined}
          >
            {showLineNumbers ? <span className={styles.lineNo}>{index + 1}</span> : null}
            <span className={styles.lineText}>
              {isEmpty
                ? ' '
                : line.map((token, tokenIndex) => (
                    <span key={tokenIndex} style={token.style as React.CSSProperties}>
                      {token.content}
                    </span>
                  ))}
            </span>
          </div>
        );
      })}
    </div>
  );
});

const ShikiCodeFence = React.memo(function ShikiCodeFence({
  content,
  language,
  isStreaming = false,
  isDiff = false,
  isDark = false,
  diffLines = EMPTY_DIFF_LINES,
  showLineNumbers = true,
}: ShikiCodeFenceProps) {
  const cacheRef = useRef(new StreamingHighlightCache());
  const hostRef = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(() => isShikiLanguageReady(language));
  const [streamLines, setStreamLines] = useState<ShikiLine[] | null>(null);

  useEffect(() => {
    if (hostRef.current) adoptBeautifulUiCodeBlockCss(hostRef.current);
  }, [ready, streamLines]);

  useEffect(() => {
    if (isShikiLanguageReady(language)) {
      setReady(true);
      return;
    }
    let cancelled = false;
    setReady(false);
    void ensureShikiLanguage(language).then(() => {
      if (!cancelled) setReady(isShikiLanguageReady(language));
    });
    return () => {
      cancelled = true;
    };
  }, [language]);

  useLayoutEffect(() => {
    if (!isStreaming) return;
    if (!ready) {
      setStreamLines(null);
      return;
    }
    setStreamLines(tokenizeCodeStreaming(content, language, true, cacheRef.current));
  }, [content, isStreaming, language, ready]);

  const lines = useMemo(() => {
    if (!ready) return null;
    if (isStreaming) return streamLines;
    return cacheRef.current.snapshotIfMatches(content, language) ?? tokenizeCode(content, language);
  }, [content, isStreaming, language, ready, streamLines]);
  const fallback = (
    <HljsCodeFence
      content={content}
      language={language}
      isDiff={isDiff}
      isDark={isDark}
      diffLines={diffLines}
      showLineNumbers={showLineNumbers}
    />
  );

  if (!lines) {
    return (
      <div ref={hostRef}>
        {fallback}
      </div>
    );
  }

  return (
    <div ref={hostRef}>
      <SyntaxHighlightBoundary fallback={fallback} resetKey={`${language}\u0000${content}`}>
        <ShikiLines
          lines={lines}
          isDiff={isDiff}
          isDark={isDark}
          diffLines={diffLines}
          showLineNumbers={showLineNumbers}
        />
      </SyntaxHighlightBoundary>
    </div>
  );
});

export default ShikiCodeFence;
