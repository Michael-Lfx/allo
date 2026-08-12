import React from 'react';

type MarketCardShellProps = {
  children: React.ReactNode;
  testId?: string;
};

/** Border-box shell prevents padding from extending a stretched grid track. */
const MarketCardShell: React.FC<MarketCardShellProps> = ({ children, testId }) => (
  <article
    data-testid={testId}
    className='box-border flex h-auto min-w-0 self-stretch flex-col rounded-16px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'
  >
    {children}
  </article>
);

export default MarketCardShell;
