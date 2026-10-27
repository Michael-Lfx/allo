import React from 'react';
import type { CSSProperties } from 'react';
import GooeyNav from '@renderer/components/base/GooeyNav';

interface FilterPillsProps<K extends string> {
  items: { key: K; label: string }[];
  active: K;
  onChange: (key: K) => void;
  ariaLabel?: string;
}

const compact: CSSProperties = {
  ['--gn-pad' as string]: '3px',
  ['--gn-y' as string]: '5px',
  ['--gn-x' as string]: '12px',
  ['--gn-font' as string]: '12px',
};

export default function FilterPills<K extends string>({
  items,
  active,
  onChange,
  ariaLabel,
}: FilterPillsProps<K>) {
  const activeIndex = items.findIndex((item) => item.key === active);
  return (
    <GooeyNav
      ariaLabel={ariaLabel}
      selection='tab'
      size='sm'
      style={compact}
      items={items.map((item) => ({ label: item.label }))}
      activeIndex={activeIndex}
      onChange={(index) => {
        const next = items[index];
        if (next && next.key !== active) onChange(next.key);
      }}
    />
  );
}
