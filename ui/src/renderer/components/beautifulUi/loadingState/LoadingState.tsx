import React from 'react';
import { formatLoadingElapsed } from './loadingStateModel';
import styles from './loadingState.module.css';

export type LoadingStateVariant = 'drive' | 'dots' | 'orbit';
export type LoadingStateProps = {
  variant: LoadingStateVariant;
  label: string;
  elapsedSeconds?: number;
};

const DRIVE_CELL_COUNT = 25;

const DriveGlyph: React.FC = () => (
  <span className={styles.drive} aria-hidden='true'>
    {Array.from({ length: DRIVE_CELL_COUNT }, (_, index) => (
      <span key={index} className={styles.driveCell} />
    ))}
  </span>
);

const DotsGlyph: React.FC = () => (
  <span className={styles.dots} aria-hidden='true'>
    <span className={styles.dot} />
    <span className={styles.dot} />
    <span className={styles.dot} />
  </span>
);

const OrbitGlyph: React.FC = () => (
  <span className={styles.orbit} aria-hidden='true'>
    <span className={styles.orbitCore} />
    <span className={styles.orbitRing}>
      <span className={styles.orbitSat} />
    </span>
  </span>
);

const variantGlyph = (variant: LoadingStateVariant): React.ReactNode => {
  switch (variant) {
    case 'drive':
      return <DriveGlyph />;
    case 'dots':
      return <DotsGlyph />;
    case 'orbit':
      return <OrbitGlyph />;
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

const LoadingState: React.FC<LoadingStateProps> = ({ variant, label, elapsedSeconds }) => (
  <div
    className={styles.root}
    data-testid='beautiful-ui-loading-state'
    data-variant={variant}
    role='status'
    aria-live='polite'
  >
    <span className={styles.glyph}>{variantGlyph(variant)}</span>
    <span className={styles.label}>{label}</span>
    {elapsedSeconds !== undefined ? (
      <span className={styles.elapsed}>{formatLoadingElapsed(elapsedSeconds)}</span>
    ) : null}
  </div>
);

export default LoadingState;
