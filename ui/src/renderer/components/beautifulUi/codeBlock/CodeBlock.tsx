import { AppMessage as Message } from '@/renderer/components/notifications';
import { Check, Copy } from 'lucide-react';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyText } from '@/renderer/utils/ui/clipboard';
import { displayNameForCodeLanguage } from './codeBlockLanguage';
import { adoptBeautifulUiCodeBlockCss } from './adoptCodeBlockCss';
import styles from './codeBlock.module.css';

export type CodeBlockProps = {
  language?: string;
  filename?: string;
  children: string;
  streaming?: boolean;
  highlighted?: React.ReactNode;
  toolbar?: React.ReactNode;
  footer?: React.ReactNode;
  hiddenCopyButton?: boolean;
};

const CodeBlock: React.FC<CodeBlockProps> = ({
  language,
  filename,
  children,
  streaming = false,
  highlighted,
  toolbar,
  footer,
  hiddenCopyButton,
}) => {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const copiedTimerRef = useRef<number | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (rootRef.current) adoptBeautifulUiCodeBlockCss(rootRef.current);
  }, []);

  useEffect(
    () => () => {
      if (copiedTimerRef.current != null) window.clearTimeout(copiedTimerRef.current);
    },
    []
  );

  const handleCopy = () => {
    void copyText(children)
      .then(() => {
        setCopied(true);
        if (copiedTimerRef.current != null) window.clearTimeout(copiedTimerRef.current);
        copiedTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {
        try {
          Message.error(t('common.copyFailed'));
        } catch {
          /* ignore */
        }
      });
  };

  const languageLabel = displayNameForCodeLanguage(language);
  const primaryLabel = filename || languageLabel;
  const secondaryLabel = filename ? languageLabel : '';
  const lines = children.length === 0 ? [''] : children.replace(/\n$/, '').split('\n');

  return (
    <div
      ref={rootRef}
      className={`${styles.root} ${streaming ? styles.streaming : ''}`.trim()}
      data-testid='beautiful-ui-code-block'
      data-streaming={streaming ? 'true' : 'false'}
    >
      <div className={styles.header}>
        <span className={styles.titles}>
          {primaryLabel ? (
            <span className={styles.filename} data-testid='beautiful-ui-code-block-language'>
              {primaryLabel}
            </span>
          ) : null}
          {secondaryLabel ? <span className={styles.language}>{secondaryLabel}</span> : null}
        </span>
        <div className={styles.toolbar}>
          {toolbar}
          {hiddenCopyButton ? null : (
            <button
              type='button'
              className={`${styles.copy}${copied ? ` ${styles.copyCopied}` : ''}`}
              data-testid='beautiful-ui-code-block-copy'
              title={copied ? t('common.copySuccess') : t('common.copy')}
              aria-label={copied ? t('common.copySuccess') : t('common.copy')}
              onClick={handleCopy}
            >
              {copied ? (
                <Check size={10} strokeWidth={3} aria-hidden />
              ) : (
                <Copy size={10} strokeWidth={2} aria-hidden />
              )}
              {copied ? t('common.copySuccess') : t('common.copy')}
            </button>
          )}
        </div>
      </div>
      {highlighted ? (
        <div className={styles.body}>{highlighted}</div>
      ) : (
        <pre className={styles.body}>
          {lines.map((line, index) => (
            <div key={index} className={styles.line}>
              <span className={styles.lineNo}>{index + 1}</span>
              <span className={styles.lineText}>{line.length > 0 ? line : ' '}</span>
            </div>
          ))}
        </pre>
      )}
      {footer}
    </div>
  );
};

export default CodeBlock;
