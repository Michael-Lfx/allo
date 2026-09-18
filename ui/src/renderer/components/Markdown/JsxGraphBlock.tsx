/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { copyText } from '@/renderer/utils/ui/clipboard';
import { Message } from '@arco-design/web-react';
import { Copy, Refresh } from '@icon-park/react';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import SyntaxHighlighter, { vs, vs2015 } from './SyntaxHighlighter';
import { repairFigure } from './figureRepair';

// The jsxgraph package's "exports" map does not expose its CSS, so the
// minimal .jxgbox rules it needs live in ShadowView's stylesheet instead.

type JsxGraphBlockProps = {
  code: string;
  style?: React.CSSProperties;
};

type JsxGraphNamespace = typeof import('jsxgraph');
// Vite's CJS interop may hand back either { default: JXG } or JXG itself.
type JsxGraphModule = JsxGraphNamespace | { default?: JsxGraphNamespace };

let jsxgraphModulePromise: Promise<JsxGraphModule> | null = null;
const loadJsxGraph = (): Promise<JsxGraphModule> => {
  jsxgraphModulePromise ??= import('jsxgraph') as Promise<JsxGraphModule>;
  return jsxgraphModulePromise;
};
const resolveJXG = (module: JsxGraphModule): JsxGraphNamespace => {
  const withDefault = module as { default?: JsxGraphNamespace };
  return withDefault.default ?? (module as JsxGraphNamespace);
};

/**
 * Render one model-authored ```jsxgraph fence. The fenced JavaScript runs
 * against a pre-created board: it may use `JXG` (the JSXGraph namespace) and
 * `board` (an initialized board it draws on, or whose bounding box it
 * replaces via board.setBoundingBox). Running it through `new Function` keeps
 * the figure self-contained while the replay button can restart scripted
 * animations from scratch.
 */
function JsxGraphBlock({ code, style }: JsxGraphBlockProps) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLDivElement>(null);
  const boardRef = useRef<import('jsxgraph').Board | null>(null);
  const [viewMode, setViewMode] = useState<'preview' | 'source'>('preview');
  const [runId, setRunId] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [debouncedCode, setDebouncedCode] = useState(code);
  // Corrected source returned by the AI repair call; while set it replaces
  // the original fence body for both execution and the source view.
  const [repairedSource, setRepairedSource] = useState<string | null>(null);
  const [repairing, setRepairing] = useState(false);
  const [currentTheme, setCurrentTheme] = useState<'light' | 'dark'>(
    () => (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light'
  );

  useEffect(() => {
    const timer = setTimeout(() => {
      // Fresh fence content invalidates any earlier repair result.
      setRepairedSource(null);
      setDebouncedCode(code);
    }, 300);
    return () => clearTimeout(timer);
  }, [code]);

  const source = repairedSource ?? debouncedCode;

  useEffect(() => {
    const update = () => {
      setCurrentTheme((document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light');
    };
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;
    const trimmed = source.trim();
    const div = canvasRef.current;
    if (!trimmed || !div) return () => {};

    setError(null);

    void loadJsxGraph()
      .then((module) => {
        if (cancelled) return;
        const JXG = resolveJXG(module);
        let board: import('jsxgraph').Board | null = null;
        const isDark = currentTheme === 'dark';
        // The bundled d.ts types `axis`/`grid` as booleans, but the runtime
        // accepts per-element sub-attributes there; cast narrowly to keep
        // the theme-aware colors.
        const boardAttributes = {
          boundingbox: [-8, 8, 8, -8],
          showCopyright: false,
          showNavigation: false,
          axis: { strokeColor: isDark ? '#9da2ab' : '#4e5969' },
          grid: {
            strokeColor: isDark ? 'rgba(255, 255, 255, 0.12)' : 'rgba(0, 0, 0, 0.08)',
          },
        } as unknown as Partial<import('jsxgraph').BoardAttributes>;
        try {
          board = JXG.JSXGraph.initBoard(div, boardAttributes);
          boardRef.current = board;
          // JSXGraph reports API misuse (e.g. "Can't create line with parent
          // types 'number' and 'number'") through console diagnostics from
          // JXG.debug instead of throwing, so capture those during the run
          // and treat them as render failures too.
          const captured: string[] = [];
          const wrapConsole = (original: (...args: unknown[]) => void) =>
            (...args: unknown[]) => {
              const text = args.map((value) => String(value)).join(' ');
              if (text.includes('JSXGraph')) captured.push(text);
              original.apply(console, args);
            };
          const originalLog = console.log;
          const originalError = console.error;
          console.log = wrapConsole(originalLog);
          console.error = wrapConsole(originalError);
          try {
            new Function('JXG', 'board', trimmed)(JXG, board);
          } finally {
            console.log = originalLog;
            console.error = originalError;
          }
          if (captured.length > 0) {
            throw new Error(captured.join('\n'));
          }
        } catch (runError) {
          if (board) {
            try {
              JXG.JSXGraph.freeBoard(board);
            } catch {
              // Board may be half-initialized; dropping the reference is enough.
            }
          }
          boardRef.current = null;
          setError(runError instanceof Error ? runError.message : String(runError));
          setViewMode('source');
        }
      })
      .catch((loadError: unknown) => {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : String(loadError));
          setViewMode('source');
        }
      });

    return () => {
      cancelled = true;
      const board = boardRef.current;
      boardRef.current = null;
      if (board) {
        void loadJsxGraph()
          .then((module) => {
            try {
              resolveJXG(module).JSXGraph.freeBoard(board);
            } catch {
              // Ignore cleanup races on unmount.
            }
          })
          .catch(() => {});
      }
    };
  }, [source, runId, currentTheme]);

  const handleRepair = useCallback(async () => {
    if (repairing) return;
    setRepairing(true);
    try {
      const response = await repairFigure({
        language: 'jsxgraph',
        code: source,
        error: error ?? '',
      });
      setRepairedSource(response.code);
      setError(null);
      setViewMode('preview');
    } catch (repairError) {
      console.error('jsxgraph figure repair failed', repairError);
      Message.error(t('preview.repairFailed'));
    } finally {
      setRepairing(false);
    }
  }, [repairing, source, error, t]);

  const codeTheme = currentTheme === 'dark' ? vs2015 : vs;
  const showPreview = viewMode === 'preview' && error === null;

  return (
    <div className='markdown-figure-block' style={style}>
      <div className='markdown-figure-surface'>
        <div className='markdown-figure-header'>
          <div className='markdown-figure-leading'>
            <span className='markdown-figure-language'>{'<jsxgraph>'}</span>
            {error === null && (
              <div className='markdown-figure-segmented' role='group' aria-label={t('preview.preview')}>
                <button
                  type='button'
                  className='markdown-figure-segment'
                  aria-pressed={viewMode === 'preview'}
                  onClick={() => setViewMode('preview')}
                >
                  {t('preview.preview')}
                </button>
                <button
                  type='button'
                  className='markdown-figure-segment'
                  aria-pressed={viewMode === 'source'}
                  onClick={() => setViewMode('source')}
                >
                  {t('preview.source')}
                </button>
              </div>
            )}
          </div>
          <div className='markdown-figure-toolbar markdown-code-toolbar'>
            <button
              type='button'
              data-testid='jsxgraph-replay'
              className='markdown-figure-action markdown-code-action'
              aria-label={t('preview.replay')}
              title={t('preview.replay')}
              onClick={() => {
                setViewMode('preview');
                setRunId((value) => value + 1);
              }}
            >
              <Refresh theme='outline' size='16' fill='currentColor' />
            </button>
            <button
              type='button'
              data-testid='jsxgraph-copy'
              className='markdown-figure-action markdown-code-action'
              aria-label={t('common.copy')}
              title={t('common.copy')}
              onClick={() => {
                void copyText(source)
                  .then(() => {
                    Message.success(t('common.copySuccess'));
                  })
                  .catch(() => {
                    Message.error(t('common.copyFailed'));
                  });
              }}
            >
              <Copy theme='outline' size='16' fill='currentColor' />
            </button>
          </div>
        </div>

        {/* The canvas stays mounted in source view so boards and running
            animations survive toggling. */}
        <div
          ref={canvasRef}
          data-testid='jsxgraph-canvas'
          className='jxgbox markdown-figure-jsxgraph'
          style={{ display: showPreview ? undefined : 'none' }}
        />
        {showPreview || (
          <div className='markdown-figure-source'>
            {error && (
              <div className='markdown-figure-error'>
                <span className='markdown-figure-error-text'>{error}</span>
                <button
                  type='button'
                  data-testid='jsxgraph-repair'
                  className='markdown-figure-segment'
                  disabled={repairing}
                  onClick={() => void handleRepair()}
                >
                  {repairing ? t('preview.repairing') : t('preview.repairWithAI')}
                </button>
              </div>
            )}
            <SyntaxHighlighter
              children={source}
              language='javascript'
              style={codeTheme}
              PreTag='div'
              customStyle={{
                margin: 0,
                borderRadius: 0,
                border: 'none',
                background: 'transparent',
                color: 'var(--text-primary)',
                overflowX: 'auto',
                maxWidth: '100%',
              }}
              codeTagProps={{ style: { color: 'var(--text-primary)' } }}
            />
          </div>
        )}
      </div>
    </div>
  );
}

export default React.memo(JsxGraphBlock);
