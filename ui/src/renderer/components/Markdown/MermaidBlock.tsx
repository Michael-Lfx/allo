

import mermaid from 'mermaid';
import SyntaxHighlighter, { vs, vs2015 } from './SyntaxHighlighter';

import { copyText } from '@/renderer/utils/ui/clipboard';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Copy, PreviewOpen } from '@icon-park/react';
import { usePreviewContextOptional } from '@/renderer/pages/conversation/Preview/context/PreviewContext';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

type MermaidBlockProps = {
  code: string;
  style?: React.CSSProperties;
  showOpenInPanelButton?: boolean;
};

let initializedTheme: 'light' | 'dark' | null = null;
const ensureMermaidInitialized = (theme: 'light' | 'dark') => {
  if (initializedTheme === theme) return;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    suppressErrorRendering: true,
    theme: theme === 'dark' ? 'dark' : 'default',
    fontFamily: 'inherit',
  });
  initializedTheme = theme;
};

const withResponsiveSvg = (svg: string): string => {
  return svg.replace(/<svg\b([^>]*)>/i, (_match, attrs: string) => {
    if (/style\s*=/.test(attrs)) {
      return `<svg${attrs.replace(
        /style\s*=\s*(["'])(.*?)\1/i,
        (_styleMatch, quote: string, styleValue: string) =>
          ` style=${quote}${styleValue};max-width: 100%; height: auto; display: block;${quote}`
      )}>`;
    }
    return `<svg${attrs} style="max-width: 100%; height: auto; display: block;">`;
  });
};

function MermaidBlock({ code, style, showOpenInPanelButton = true }: MermaidBlockProps) {
  const { t } = useTranslation();
  // 可选地消费预览上下文：mermaid 块可能渲染在 ChatLayout（预览 provider）之外
  // 的表面（桌宠 / 知识库 / 设定 / 更新说明等），此时 context 为 null，仍内联渲染
  // 图表，仅隐藏「在预览面板打开」入口。
  // Optionally consume the preview context: a mermaid block can render outside
  // ChatLayout's preview provider (companion / knowledge / presets / release
  // notes), where context is null — we still render the diagram inline and only
  // hide the "open in preview panel" affordance.
  const previewContext = usePreviewContextOptional();
  const canOpenInPanel = previewContext !== null;
  const blockIdRef = useRef(`mermaid-${Math.random().toString(36).slice(2, 10)}`);
  const preferredViewModeRef = useRef<'preview' | 'source' | null>(null);
  const [svg, setSvg] = useState<string | null>(null);
  const [isRendering, setIsRendering] = useState(false);
  const [viewMode, setViewMode] = useState<'preview' | 'source'>('source');
  const [debouncedCode, setDebouncedCode] = useState(code);
  const [currentTheme, setCurrentTheme] = useState<'light' | 'dark'>(() => {
    return (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light';
  });

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedCode(code), 300);
    return () => clearTimeout(timer);
  }, [code]);

  useEffect(() => {
    const updateTheme = () => {
      const theme = (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light';
      setCurrentTheme(theme);
    };

    const observer = new MutationObserver(updateTheme);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });

    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;
    const source = debouncedCode.trim();

    if (!source) {
      setSvg(null);
      setIsRendering(false);
      setViewMode('source');
      return () => {
        cancelled = true;
      };
    }

    setSvg(null);
    setIsRendering(true);

    const renderDiagram = async () => {
      try {
        ensureMermaidInitialized(currentTheme);

        const { svg: renderedSvg } = await mermaid.render(`${blockIdRef.current}-${Date.now()}`, source);

        if (!cancelled) {
          setSvg(withResponsiveSvg(renderedSvg));
          setIsRendering(false);
          setViewMode(preferredViewModeRef.current === 'source' ? 'source' : 'preview');
        }
      } catch {
        if (!cancelled) {
          setSvg(null);
          setIsRendering(false);
          setViewMode('source');
        }
      }
    };

    void renderDiagram();

    return () => {
      cancelled = true;
    };
  }, [debouncedCode, currentTheme]);

  const codeTheme = currentTheme === 'dark' ? vs2015 : vs;
  const shouldShowLoading = isRendering && preferredViewModeRef.current !== 'source';
  const summary = code
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  const previewTitle =
    summary && summary.length > 0
      ? `${t('preview.mermaidTitle')}: ${summary.slice(0, 48)}${summary.length > 48 ? '...' : ''}`
      : t('preview.mermaidTitle');

  return (
    <div className='markdown-mermaid-block' style={style}>
      <div className='markdown-mermaid-surface'>
        <div className='markdown-mermaid-header'>
          <div className='markdown-mermaid-leading'>
            <span className='markdown-mermaid-language'>
              {'<mermaid>'}
            </span>
            {svg && (
              <div className='markdown-mermaid-segmented' role='group' aria-label={t('preview.preview')}>
                <button
                  type='button'
                  className='markdown-mermaid-segment'
                  aria-pressed={viewMode === 'preview'}
                  onClick={() => {
                    preferredViewModeRef.current = 'preview';
                    setViewMode('preview');
                  }}
                >
                  {t('preview.preview')}
                </button>
                <button
                  type='button'
                  className='markdown-mermaid-segment'
                  aria-pressed={viewMode === 'source'}
                  onClick={() => {
                    preferredViewModeRef.current = 'source';
                    setViewMode('source');
                  }}
                >
                  {t('preview.source')}
                </button>
              </div>
            )}
          </div>
          <div className='markdown-mermaid-toolbar markdown-code-toolbar'>
            {showOpenInPanelButton && canOpenInPanel && (
              <button
                type='button'
                data-testid='mermaid-open-in-panel'
                className='markdown-mermaid-action markdown-code-action'
                aria-label={t('preview.openInPanelTooltip')}
                title={t('preview.openInPanelTooltip')}
                onClick={() => {
                  previewContext?.openPreview(`\`\`\`mermaid\n${code}\n\`\`\``, 'markdown', {
                    title: previewTitle,
                    editable: false,
                  });
                }}
              >
                <PreviewOpen theme='outline' size='16' fill='currentColor' />
              </button>
            )}
            <button
              type='button'
              data-testid='mermaid-copy'
              className='markdown-mermaid-action markdown-code-action'
              aria-label={t('common.copy')}
              title={t('common.copy')}
              onClick={() => {
                void copyText(code)
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

        {svg && viewMode === 'preview' ? (
          <div
            data-testid='mermaid-diagram'
            className='markdown-mermaid-diagram'
            dangerouslySetInnerHTML={{ __html: svg }}
          />
        ) : shouldShowLoading ? (
          <div
            data-testid='mermaid-loading'
            className='markdown-mermaid-loading'
          >
            <div
              aria-hidden='true'
              className='markdown-mermaid-loading-indicator loading'
            />
            <span>{t('preview.loading')}</span>
          </div>
        ) : (
          <SyntaxHighlighter
            children={code}
            language='mermaid'
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
        )}
      </div>
    </div>
  );
}

export default React.memo(MermaidBlock);
