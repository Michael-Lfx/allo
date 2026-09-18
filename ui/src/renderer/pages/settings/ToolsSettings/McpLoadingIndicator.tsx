import React from 'react';
import styles from './McpLoadingIndicator.module.css';

export type McpLoadingIndicatorProps = {
  size?: 'small' | 'medium';
  label?: string;
  className?: string;
};

/**
 * A deliberately small, CSS-driven indicator for MCP requests.
 * The parent owns status semantics; this component only renders motion.
 */
const McpLoadingIndicator: React.FC<McpLoadingIndicatorProps> = ({ size = 'small', label, className }) => {
  const classes = [styles.root, styles[size], className].filter(Boolean).join(' ');

  return (
    <span className={classes}>
      <span className={styles.ring} aria-hidden='true' />
      {label ? <span className='truncate'>{label}</span> : null}
    </span>
  );
};

export default McpLoadingIndicator;
