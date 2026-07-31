

import React from 'react';

/**
 * Preference row component
 * Displays a label and control in a unified horizontal layout
 */
const PreferenceRow: React.FC<{
  label: string;
  children: React.ReactNode;
  description?: string;
}> = ({ label, children, description }) => (
  <div className='flex flex-col items-stretch gap-8px py-12px sm:flex-row sm:items-center sm:justify-between sm:gap-24px'>
    <div className='min-w-0 flex-1'>
      <div className='text-14px text-2'>{label}</div>
      {description && <div className='text-12px text-t-tertiary mt-4px'>{description}</div>}
    </div>
    <div className='flex w-full shrink-0 justify-end sm:w-auto'>{children}</div>
  </div>
);

export default PreferenceRow;
