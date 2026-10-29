import React from 'react';

interface SidebarToggleIconProps {
  collapsed: boolean;
  size?: number;
}

/** One sidebar glyph for both expanded and collapsed states. */
const SidebarToggleIcon: React.FC<SidebarToggleIconProps> = ({ size = 15 }) => {
  const height = size;
  const width = size + 2;
  const viewBoxWidth = (24 * width) / height;
  const rectWidth = 18 + (viewBoxWidth - 24);

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${viewBoxWidth} 24`}
      fill='none'
      stroke='currentColor'
      strokeWidth={1.5}
      strokeLinecap='round'
      strokeLinejoin='round'
      aria-hidden='true'
      focusable='false'
    >
      <rect x='3' y='2' width={rectWidth} height='20' rx='3' />
      <line x1='9' y1='2' x2='9' y2='22' />
    </svg>
  );
};

export default SidebarToggleIcon;
