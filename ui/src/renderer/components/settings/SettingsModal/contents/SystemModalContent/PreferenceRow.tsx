

import React from 'react';
import {
  type SettingsRowProps,
  SettingsRow,
} from '@/renderer/components/settings/SettingsPagePrimitives';

/**
 * Preference row component
 * Displays a label and control in a unified horizontal layout
 */
const PreferenceRow: React.FC<{
  label: string;
  children: React.ReactNode;
  description?: string;
  controlLayout?: SettingsRowProps['controlLayout'];
}> = ({ label, children, description, controlLayout = 'compact' }) => (
  <SettingsRow
    label={label}
    description={description}
    control={children}
    controlLayout={controlLayout}
  />
);

export default PreferenceRow;
