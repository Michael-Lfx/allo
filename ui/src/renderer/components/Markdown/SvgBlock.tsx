/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import DOMPurify from 'dompurify';

import { copyText } from '@/renderer/utils/ui/clipboard';
import { Message } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import SyntaxHighlighter, { vs, vs2015 } from './SyntaxHighlighter';
import { repairFigure } from './figureRepair';

type SvgBlockProps = {
  code: string;
  style?: React.CSSProperties;
};

/**
 * Sanitize a model-authored SVG fence. The SVG profile strips scripts, event
 * handlers and foreign content; SMIL animation elements stay allowed, which
 * is how lesson figures express simple repeating step animations.
 */
const sanitizeSvg = (source: string): string | null => {
  const cleaned = DOMPurify.sanitize(source, {
    USE_PROFILES: { svg: true, svgFilters: true },
  });
  return /<svg[\s>]/i.test(cleaned) ? cleaned : null;
};

function SvgBlock({ code, style }: SvgBlockProps) {
  const { t } = useTranslation();
  const [viewMode, setViewMode] = useState<'preview' | 'source'>('preview');
  // Corrected source returned by the AI repair call; while set it replaces
  // the original fence body for both rendering and the source view.
  const [repairedSource, setRepairedSource] = useState<string | null>(null);
  const [repairing, setRepairing] = useState(false);
  const [currentTheme, setCurrentTheme] = useState<'light' | 'dark'>(
    () => (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light'
  );

  useEffect(() => {
    const update = () => {
      setCurrentTheme((document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light');
    };
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);

  // Fresh fence content invalidates any earlier repair result.
  useEffect(() => {
    setRepairedSource(null);
  }, [code]);

  const source = repairedSource ?? code;
  const sanitized = useMemo(() => sanitizeSvg(source), [source]);

  const handleRepair = useCallback(async () => {
    if (repairing) return;
    setRepairing(true);
    try {
      const response = await repairFigure({
        language: 'svg',
        code: source,
        error: 'SVG failed to render: sanitization dropped the <svg> element or the body is empty',
      });
      setRepairedSource(response.code);
      setViewMode('preview');
    } catch (repairError) {
      console.error('svg figure repair failed', repairError);
      Message.error(t('preview.repairFailed'));
    } finally {
      setRepairing(false);
    }
  }, [repairing, source, t]);

  const codeTheme = currentTheme === 'dark' ? vs2015 : vs;

  return (
    <div className='markdown-figure-block' style={style}>
      <div className='markdown-figure-surface'>
        <div className='markdown-figure-header'>
          <div className='markdown-figure-leading'>
            <span className='markdown-figure-language'>{'<svg>'}</span>
            {sanitized && (
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
              data-testid='svg-copy'
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

        {sanitized && viewMode === 'preview' ? (
          <div
            data-testid='svg-diagram'
            className='markdown-figure-svg'
            dangerouslySetInnerHTML={{ __html: sanitized }}
          />
        ) : (
          <div className='markdown-figure-source'>
            {!sanitized && (
              <div className='markdown-figure-error'>
                <span className='markdown-figure-error-text'>
                  {t('preview.svgSanitizeFailed')}
                </span>
                <button
                  type='button'
                  data-testid='svg-repair'
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
              language='xml'
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

export default React.memo(SvgBlock);
