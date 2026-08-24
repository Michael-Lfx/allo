import React from 'react';

interface SidebarToggleIconProps {
  collapsed: boolean;
  size?: number;
  strokeWidth?: number;
}

/** The sidebar outline stays stable while the caret communicates the next action. */
const SidebarToggleIcon: React.FC<SidebarToggleIconProps> = ({ collapsed, size = 18, strokeWidth = 4 }) => (
  <svg
    width={size}
    height={size}
    viewBox='0 0 48 48'
    fill='none'
    stroke='currentColor'
    strokeWidth={strokeWidth}
    strokeLinecap='round'
    strokeLinejoin='round'
    aria-hidden='true'
    focusable='false'
  >
    <rect x='6' y='10' width='36' height='28' rx='5' />
    <line x1='18' y1='10' x2='18' y2='38' />
    <path
      className='sidebar-toggle-icon__caret'
      d='M30 18 24 24l6 6'
      style={{ transform: collapsed ? 'rotate(180deg)' : 'rotate(0deg)' }}
    />
  </svg>
);

export default SidebarToggleIcon;
