/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useState } from 'react';
import { isMonochromeLogo } from '@/renderer/utils/model/modelLogos';

export interface ModelBrandIconProps {
  /** Built-in vendor logo URL from the showcase registry; empty renders nothing. */
  src: string;
  className?: string;
}

/**
 * Vendor logo for a model row. Renders nothing for an unknown brand or a
 * failed load — the row then keeps its text-only layout instead of showing a
 * broken-image placeholder.
 */
const ModelBrandIcon: React.FC<ModelBrandIconProps> = ({ src, className = '' }) => {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [src]);

  if (!src || failed) return null;
  const monochrome = isMonochromeLogo(src);
  return (
    <img
      src={src}
      alt=''
      referrerPolicy='no-referrer'
      className={`h-16px w-16px shrink-0 object-contain ${monochrome ? 'dark:invert' : ''} ${className}`.trim()}
      onError={() => setFailed(true)}
    />
  );
};

export default ModelBrandIcon;
