import { ChevronDown } from 'lucide-react';
import React, { useState } from 'react';
import styles from './diffTable.module.css';

export type DiffTableFile = {
  id: string;
  title: string;
  insertions: number;
  deletions: number;
};

export type DiffTableProps = {
  title?: string;
  files: DiffTableFile[];
  defaultExpanded?: boolean;
  onFileClick?: (file: DiffTableFile) => void;
  onDiffClick?: (file: DiffTableFile) => void;
  className?: string;
};

const DiffTable: React.FC<DiffTableProps> = ({
  title,
  files,
  defaultExpanded = true,
  onFileClick,
  onDiffClick,
  className,
}) => {
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <div
      className={`${styles.root}${className ? ` ${className}` : ''}`.trim()}
      data-testid='beautiful-ui-diff-table'
    >
      {title ? (
        <button
          type='button'
          className={styles.header}
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          <span className={styles.headerTitle}>{title}</span>
          <ChevronDown
            size={16}
            strokeWidth={1.75}
            aria-hidden
            className={`${styles.chevron}${expanded ? ` ${styles.chevronOpen}` : ''}`}
          />
        </button>
      ) : null}
      {expanded ? (
        <ul className={styles.list}>
          {files.map((file) => (
            <li key={file.id} className={styles.row}>
              <button
                type='button'
                className={styles.filename}
                title={file.id}
                onClick={() => onFileClick?.(file)}
              >
                {file.title}
              </button>
              <button type='button' className={styles.stats} onClick={() => onDiffClick?.(file)}>
                <span className={styles.insertions}>+{file.insertions}</span>
                <span className={styles.deletions}>-{file.deletions}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
};

export default DiffTable;
