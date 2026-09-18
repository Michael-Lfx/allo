import { ChevronDown } from 'lucide-react';
import React, { useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  INLINE_DIFF_COLLAPSE_LINE_THRESHOLD,
  countDiffLines,
  type InlineDiffHunk,
  type InlineDiffLine,
} from './inlineDiffModel';
import styles from './inlineDiff.module.css';

export type InlineDiffProps = {
  filename: string;
  hunks: InlineDiffHunk[];
  insertions: number;
  deletions: number;
  defaultExpanded?: boolean;
  collapsedLineLimit?: number;
  className?: string;
};

const lineClass = (kind: InlineDiffLine['kind']): string => {
  switch (kind) {
    case 'insert':
      return `${styles.line} ${styles.insert}`;
    case 'delete':
      return `${styles.line} ${styles.delete}`;
    default:
      return styles.line;
  }
};

const lineMarker = (kind: InlineDiffLine['kind']): string => {
  switch (kind) {
    case 'insert':
      return '+';
    case 'delete':
      return '-';
    default:
      return ' ';
  }
};

const lineNumber = (line: InlineDiffLine): string => {
  const value = line.kind === 'delete' ? line.oldLine : line.newLine ?? line.oldLine;
  return value == null ? '' : String(value);
};

const DiffLine: React.FC<{ line: InlineDiffLine }> = ({ line }) => (
  <div className={lineClass(line.kind)}>
    <span className={styles.gutter}>{lineNumber(line)}</span>
    <span className={styles.marker}>{lineMarker(line.kind)}</span>
    <span className={styles.text}>{line.text || ' '}</span>
  </div>
);

const InlineDiff: React.FC<InlineDiffProps> = ({
  filename,
  hunks,
  insertions,
  deletions,
  defaultExpanded,
  collapsedLineLimit = INLINE_DIFF_COLLAPSE_LINE_THRESHOLD,
  className,
}) => {
  const { t } = useTranslation();
  const bodyId = useId();
  const totalLines = countDiffLines(hunks);
  const [expanded, setExpanded] = useState(
    defaultExpanded ?? totalLines <= collapsedLineLimit
  );
  const [showAllLines, setShowAllLines] = useState(totalLines <= collapsedLineLimit);

  const visibleHunks = useMemo(() => {
    if (!expanded) return [];
    if (showAllLines || totalLines <= collapsedLineLimit) return hunks;

    let remaining = collapsedLineLimit;
    const clipped: InlineDiffHunk[] = [];
    for (const hunk of hunks) {
      if (remaining <= 0) break;
      if (hunk.lines.length <= remaining) {
        clipped.push(hunk);
        remaining -= hunk.lines.length;
        continue;
      }
      clipped.push({ header: hunk.header, lines: hunk.lines.slice(0, remaining) });
      break;
    }
    return clipped;
  }, [collapsedLineLimit, expanded, hunks, showAllLines, totalLines]);

  const hiddenLineCount = expanded && !showAllLines ? Math.max(0, totalLines - collapsedLineLimit) : 0;

  return (
    <div
      className={`${styles.root}${className ? ` ${className}` : ''}`.trim()}
      data-testid='beautiful-ui-inline-diff'
    >
      <button
        type='button'
        className={styles.header}
        aria-expanded={expanded}
        aria-controls={bodyId}
        aria-label={expanded ? t('common.collapse') : t('common.expandMore')}
        onClick={() => setExpanded((value) => !value)}
      >
        <span className={styles.titles}>
          <span className={styles.filename} title={filename}>
            {filename}
          </span>
        </span>
        <span className={styles.meta}>
          {insertions > 0 ? <span className={styles.insertions}>+{insertions}</span> : null}
          {deletions > 0 ? <span className={styles.deletions}>-{deletions}</span> : null}
          <ChevronDown
            size={16}
            strokeWidth={1.75}
            aria-hidden
            className={`${styles.chevron}${expanded ? ` ${styles.chevronOpen}` : ''}`}
          />
        </span>
      </button>
      {expanded ? (
        <div id={bodyId} className={styles.body}>
          {visibleHunks.map((hunk, index) => (
            <div key={`${hunk.header}-${index}`} className={styles.hunk}>
              <div className={styles.hunkHeader}>{hunk.header}</div>
              {hunk.lines.map((line, lineIndex) => (
                <DiffLine key={`${line.kind}-${line.oldLine ?? ''}-${line.newLine ?? ''}-${lineIndex}`} line={line} />
              ))}
            </div>
          ))}
          {hiddenLineCount > 0 ? (
            <button
              type='button'
              className={styles.more}
              onClick={() => setShowAllLines(true)}
            >
              {t('common.viewMoreLines', { count: hiddenLineCount })}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
};

export default InlineDiff;
