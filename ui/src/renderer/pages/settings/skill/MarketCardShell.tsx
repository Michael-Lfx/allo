import classNames from 'classnames';
import React from 'react';
import styles from './MarketCardShell.module.css';

type MarketCardShellProps = {
  children: React.ReactNode;
  testId?: string;
  /**
   * Pointer-follow tilt and a light band. A catalog cannot give every card its
   * own WebGL context or a camera, so the metal plate stays in CSS.
   */
  reflective?: boolean;
};

const reducedMotion = () =>
  typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const aimPlate = (event: React.PointerEvent<HTMLElement>) => {
  if (reducedMotion()) return;
  const rect = event.currentTarget.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const nx = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  const ny = ((event.clientY - rect.top) / rect.height) * 2 - 1;
  const plate = event.currentTarget;
  plate.style.setProperty('--rc-rx', `${(-ny * 4).toFixed(2)}deg`);
  plate.style.setProperty('--rc-ry', `${(nx * 5).toFixed(2)}deg`);
  plate.style.setProperty('--rc-gx', `${((nx + 1) * 50).toFixed(1)}%`);
  plate.style.setProperty('--rc-gy', `${((ny + 1) * 50).toFixed(1)}%`);
  plate.style.setProperty('--rc-glare', '1');
};

const releasePlate = (event: React.PointerEvent<HTMLElement>) => {
  const plate = event.currentTarget;
  plate.style.setProperty('--rc-rx', '0deg');
  plate.style.setProperty('--rc-ry', '0deg');
  plate.style.setProperty('--rc-glare', '0');
};

/** Border-box shell prevents padding from extending a stretched grid track. */
const MarketCardShell: React.FC<MarketCardShellProps> = ({ children, testId, reflective = false }) => {
  const card = (
    <article
      data-testid={testId}
      className={classNames(
        'box-border flex h-auto min-w-0 self-stretch flex-col rounded-16px border border-solid border-[var(--color-border-2)] p-16px hover:border-[var(--color-border-3)] motion-reduce:transition-none',
        reflective ? styles.plate : 'bg-[var(--color-bg-2)] transition-colors duration-150'
      )}
      onPointerMove={reflective ? aimPlate : undefined}
      onPointerLeave={reflective ? releasePlate : undefined}
    >
      {reflective && (
        <>
          <span className={styles.glare} aria-hidden='true' />
          <span className={styles.rim} aria-hidden='true' />
        </>
      )}
      <div className={reflective ? styles.content : 'contents'}>{children}</div>
    </article>
  );

  if (!reflective) return card;
  return <div className={styles.stage}>{card}</div>;
};

export default MarketCardShell;
