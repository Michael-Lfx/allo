import CodeMirror, { type ReactCodeMirrorProps } from '@uiw/react-codemirror';
import { history, historyKeymap } from '@codemirror/commands';
import { css } from '@codemirror/lang-css';
import { html } from '@codemirror/lang-html';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import type { Extension } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

export type CodeMirrorLanguage = 'css' | 'html' | 'json' | 'markdown';

export interface CodeMirrorExtensionOptions {
  language?: CodeMirrorLanguage;
  lineWrapping?: boolean;
  withHistory?: boolean;
}

/** Create all product-supported language/editor extensions from one module. */
export function createCodeMirrorExtensions({
  language,
  lineWrapping = false,
  withHistory = false,
}: CodeMirrorExtensionOptions): Extension[] {
  const languageExtension = (() => {
    switch (language) {
      case 'css':
        return css();
      case 'html':
        return html();
      case 'json':
        return json();
      case 'markdown':
        return markdown();
      default:
        return null;
    }
  })();

  return [
    ...(languageExtension ? [languageExtension] : []),
    ...(lineWrapping ? [EditorView.lineWrapping] : []),
    ...(withHistory ? [history(), keymap.of(historyKeymap)] : []),
  ];
}

interface CodeMirrorErrorBoundaryProps {
  children: React.ReactNode;
  onRetry: () => void;
  onClose?: () => void;
  title: string;
  description: string;
  retryLabel: string;
  closeLabel: string;
}

interface CodeMirrorErrorBoundaryState {
  error: Error | null;
}

class CodeMirrorErrorBoundary extends React.Component<
  CodeMirrorErrorBoundaryProps,
  CodeMirrorErrorBoundaryState
> {
  state: CodeMirrorErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): CodeMirrorErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    // Keep the exact runtime extension error available for packaged-app
    // diagnostics while limiting the failure to this editor instance.
    console.error('[CodeMirrorErrorBoundary] editor crashed:', error, info.componentStack);
  }

  render(): React.ReactNode {
    if (!this.state.error) return this.props.children;

    return (
      <div
        role='alert'
        className='flex flex-col gap-8px rounded-8px border border-[var(--color-danger-light-2)] bg-[var(--color-danger-light-1)] p-12px'
      >
        <div className='text-13px font-medium text-danger'>{this.props.title}</div>
        <div className='text-12px text-t-secondary'>{this.props.description}</div>
        <div className='flex gap-8px'>
          <button
            type='button'
            className='rounded-6px border border-[var(--color-danger)] bg-transparent px-10px py-4px text-12px text-danger cursor-pointer'
            onClick={this.props.onRetry}
          >
            {this.props.retryLabel}
          </button>
          {this.props.onClose ? (
            <button
              type='button'
              className='rounded-6px border border-[var(--color-border-2)] bg-transparent px-10px py-4px text-12px text-t-secondary cursor-pointer'
              onClick={this.props.onClose}
            >
              {this.props.closeLabel}
            </button>
          ) : null}
        </div>
        <details className='text-11px text-t-tertiary'>
          <summary>{this.state.error.message}</summary>
          {this.state.error.stack ? <pre className='mt-6px whitespace-pre-wrap'>{this.state.error.stack}</pre> : null}
        </details>
      </div>
    );
  }
}

export type CodeMirrorEditorProps = Omit<ReactCodeMirrorProps, 'extensions'> & {
  onClose?: () => void;
  extensions?: Extension[];
  language?: CodeMirrorLanguage;
  lineWrapping?: boolean;
  withHistory?: boolean;
};

/**
 * Single renderer entry point for CodeMirror.
 *
 * Keeping the wrapper at one seam makes dependency deduplication and the
 * local recovery behaviour apply to settings, preview, and import editors
 * together. A retry remounts the underlying editor so a bad extension set
 * cannot remain captured in a half-created view.
 */
export const CodeMirrorEditor: React.FC<CodeMirrorEditorProps> = ({
  onClose,
  extensions,
  language,
  lineWrapping,
  withHistory,
  ...editorProps
}) => {
  const { t } = useTranslation();
  const [attempt, setAttempt] = useState(0);
  const retry = () => setAttempt((value) => value + 1);
  const editorExtensions = useMemo(
    () => [...createCodeMirrorExtensions({ language, lineWrapping, withHistory }), ...(extensions ?? [])],
    [extensions, language, lineWrapping, withHistory]
  );

  return (
    <CodeMirrorErrorBoundary
      key={attempt}
      onRetry={retry}
      onClose={onClose}
      title={t('common.codeEditorErrorTitle')}
      description={t('common.codeEditorErrorDescription')}
      retryLabel={t('common.retry')}
      closeLabel={t('common.close')}
    >
      <CodeMirror key={attempt} {...editorProps} extensions={editorExtensions} />
    </CodeMirrorErrorBoundary>
  );
};

export default CodeMirrorEditor;
