import React from 'react';

type MarketCardGridProps = {
  children: React.ReactNode;
  busy?: boolean;
};

/** Shared responsive grid. Rows stretch independently; cards never need h-full. */
const MarketCardGrid: React.FC<MarketCardGridProps> = ({ children, busy = false }) => (
  <div
    aria-busy={busy}
    className='grid min-w-0 items-stretch gap-16px transition-opacity duration-120 motion-reduce:transition-none [&>*]:[content-visibility:auto] [&>*]:[contain-intrinsic-size:auto_200px]'
    style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(280px, 100%), 1fr))' }}
  >
    {children}
  </div>
);

export default MarketCardGrid;
