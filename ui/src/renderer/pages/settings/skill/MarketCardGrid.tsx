import React from 'react';

type MarketCardGridProps = {
  children: React.ReactNode;
  busy?: boolean;
};

/** Shared responsive grid. Rows stretch independently; cards never need h-full. */
const MarketCardGrid: React.FC<MarketCardGridProps> = ({ children, busy = false }) => (
  <div
    aria-busy={busy}
    className='grid min-w-0 items-stretch gap-12px transition-opacity duration-120 motion-reduce:transition-none'
    style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(270px, 100%), 1fr))' }}
  >
    {children}
  </div>
);

export default MarketCardGrid;
