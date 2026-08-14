import React from 'react';
import styles from './streamingText.module.css';

export type StreamingTextStatus = 'streaming' | 'done';

export type StreamingTextProps = {
  status: StreamingTextStatus;
  children: React.ReactNode;
  sourcesLabel?: string;
};

const statusClass = (status: StreamingTextStatus): string => {
  switch (status) {
    case 'streaming':
      return styles.streaming;
    case 'done':
      return '';
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const StreamingText: React.FC<StreamingTextProps> = ({ status, children, sourcesLabel }) => (
  <div
    className={`${styles.root} ${statusClass(status)}`.trim()}
    data-testid='beautiful-ui-streaming-text'
    data-status={status}
  >
    {sourcesLabel ? <p className={styles.sources}>{sourcesLabel}</p> : null}
    <div className={styles.body}>{children}</div>
  </div>
);

export default StreamingText;
