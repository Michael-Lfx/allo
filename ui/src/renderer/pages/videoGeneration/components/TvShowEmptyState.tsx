import React from 'react';

export default function TvShowEmptyState({
  title,
  desc,
  action,
}: {
  title: string;
  desc: string;
  action?: React.ReactNode;
}) {
  return (
    <div className='flex flex-col items-center gap-10px py-48px text-center'>
      <div className='text-18px font-650 text-[var(--color-text-1)]'>{title}</div>
      <div className='max-w-420px text-13px text-[var(--color-text-3)]'>{desc}</div>
      {action}
    </div>
  );
}
